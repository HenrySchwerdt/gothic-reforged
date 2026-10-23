//! Debug: survey property sets of given classes in the world documents.
//! Usage: class_survey <Data dir> <class>[,<class>...] [max-entities]
use gothic_formats::{propstream::DecodeStats, vfs::VirtualFileSystem, world_node::WorldDocument};
use std::collections::BTreeMap;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let classes: Vec<String> = a[2].split(',').map(str::to_string).collect();
    let max: usize = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(5);
    let mut vfs = VirtualFileSystem::open_data_dir(std::path::Path::new(&a[1])).expect("vfs");
    let slots: Vec<_> = vfs
        .iter()
        .filter(|(p, _)| {
            let l = p.to_ascii_lowercase();
            l.ends_with(".node") || l.ends_with(".lrentdat")
        })
        .map(|(p, s)| (p.to_string(), s))
        .collect();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut shown: BTreeMap<String, usize> = BTreeMap::new();
    for (path, slot) in slots {
        let Ok(bytes) = vfs.read(slot) else { continue };
        let want = |c: &str| classes.iter().any(|k| k == c);
        if !classes.iter().any(|k| {
            bytes
                .windows(k.len().min(16))
                .any(|w| w == &k.as_bytes()[..k.len().min(16)])
        }) {
            continue;
        }
        let mut stats = DecodeStats::default();
        let Ok((doc, sets)) =
            WorldDocument::parse_with_properties(&bytes, &|c| want(c), &mut stats)
        else {
            continue;
        };
        for (e, s) in doc.entities.iter().zip(&sets) {
            for p in s.iter().filter(|p| want(&p.class)) {
                *counts.entry(p.class.clone()).or_default() += 1;
                let n = shown.entry(p.class.clone()).or_default();
                if *n < max {
                    *n += 1;
                    println!("== {} in {} pos {:?}", p.class, path, e.world_matrix[3]);
                    for pr in &p.properties {
                        println!("   {} = {:?}", pr.name, pr.value);
                    }
                }
            }
        }
    }
    println!("{counts:?}");
}
