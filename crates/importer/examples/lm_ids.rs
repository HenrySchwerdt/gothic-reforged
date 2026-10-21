//! Debug: relate world entity IDs to Lightmaps.pak GUID file names.
use gothic_formats::{PakArchive, vfs::VirtualFileSystem};
use gothic_importer::world_index::WorldIndex;
use std::collections::HashSet;
fn main() {
    let dd = std::path::Path::new("D:/SteamLibrary/steamapps/common/Gothic 3/Data");
    let mut vfs = VirtualFileSystem::open_data_dir(dd).unwrap();
    let idx = WorldIndex::build(&mut vfs).unwrap();
    let pak = PakArchive::open(dd.join("Lightmaps.pak")).unwrap();
    let mut guids: HashSet<String> = HashSet::new();
    for e in &pak.entries {
        if let (Some(a), Some(b)) = (e.path.find('{'), e.path.find('}')) {
            guids.insert(e.path[a + 1..b].replace('-', "").to_ascii_lowercase());
        }
    }
    println!("guids {}", guids.len());
    let mut shown = 0;
    let (mut n, mut hit) = ([0usize; 6], 0usize);
    for r in idx.records() {
        let id = &r.entity_id;
        n[0] += 1;
        if shown < 3 && r.resource.to_ascii_lowercase().contains("bridge_small_01") {
            println!("{} {}", id, r.resource);
            shown += 1;
        }
        let bytes: Vec<u8> = (0..20)
            .map(|i| u8::from_str_radix(&id[i * 2..i * 2 + 2], 16).unwrap())
            .collect();
        for off in [0usize, 4] {
            let g = &bytes[off..off + 16];
            let be: String = g.iter().map(|b| format!("{b:02x}")).collect();
            let mut m = g.to_vec();
            m[0..4].reverse();
            m[4..6].reverse();
            m[6..8].reverse();
            let ms: String = m.iter().map(|b| format!("{b:02x}")).collect();
            if guids.contains(&be) {
                n[1 + off / 4 * 2] += 1;
            }
            if guids.contains(&ms) {
                n[2 + off / 4 * 2] += 1;
            }
        }
        let _ = &mut hit;
    }
    println!(
        "records {} be0 {} ms0 {} be4 {} ms4 {}",
        n[0], n[1], n[2], n[3], n[4]
    );
}
