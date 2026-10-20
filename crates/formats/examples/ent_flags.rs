//! Debug: eCEntity Enabled / ProcessingDisabled flags per document. Usage: ent_flags DATA_DIR SUBSTR [NAMESUBSTR]
use gothic_formats::vfs::VirtualFileSystem;
use gothic_formats::world_node::WorldDocument;
use std::path::Path;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut vfs = VirtualFileSystem::open_data_dir(Path::new(&a[1])).unwrap();
    let f = a[2].to_ascii_lowercase();
    let n = a.get(3).map(|s| s.to_ascii_lowercase());
    let slots: Vec<_> = vfs
        .iter()
        .filter(|(p, _)| {
            let l = p.to_ascii_lowercase();
            (l.ends_with(".node") || l.ends_with(".lrentdat")) && l.contains(&f)
        })
        .map(|(p, s)| (p.to_owned(), s))
        .collect();
    for (path, slot) in slots {
        let bytes = vfs.read(slot).unwrap();
        let Ok(doc) = WorldDocument::parse(&bytes) else { continue };
        let dis = doc.entities.iter().filter(|e| !e.enabled).count();
        let pd = doc.entities.iter().filter(|e| e.processing_disabled).count();
        println!("{path}: {} entities, {dis} disabled, {pd} processing-disabled", doc.entities.len());
        if let Some(n) = &n {
            for e in doc.entities.iter().filter(|e| e.name.to_ascii_lowercase().contains(n)) {
                println!("  {} enabled={} procdis={}", e.name, e.enabled, e.processing_disabled);
            }
        }
    }
}
