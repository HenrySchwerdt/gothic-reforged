use gothic_formats::{vfs::VirtualFileSystem, terrain::vegetation};
fn main() {
    let mut vfs = VirtualFileSystem::open_data_dir(std::path::Path::new(r"D:\SteamLibrary\steamapps\common\Gothic 3\Data")).unwrap();
    let docs: Vec<String> = vfs.iter().filter(|(p, _)| p.to_lowercase().ends_with("_cstat.node")).map(|(p, _)| p.to_string()).collect();
    let mut shown = 0;
    for d in &docs {
        let slot = vfs.resolve(d).unwrap();
        let v = vegetation::extract(&vfs.read(slot).unwrap()).unwrap();
        for ar in &v.areas {
            for m in ar.meshes.iter().take(4) {
                if shown < 24 { shown += 1;
                    let h: String = m.header.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ");
                    println!("{} {} hdr {h} pairs0 {:?} v0 {:?}", d.rsplit('_').nth(1).unwrap(), m.mesh, m.pairs[0], m.positions[0]);
                }
            }
        }
    }
}
