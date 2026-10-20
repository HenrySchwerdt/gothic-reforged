use gothic_formats::vfs::VirtualFileSystem;
use gothic_importer::world_index::WorldIndex;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let pat = a[1].to_lowercase();
    let (cx, cz, r): (f32, f32, f32) = (a[2].parse().unwrap(), a[3].parse().unwrap(), a[4].parse().unwrap());
    let mut vfs = VirtualFileSystem::open_data_dir(std::path::Path::new(r"D:\SteamLibrary\steamapps\common\Gothic 3\Data")).unwrap();
    let idx = WorldIndex::build(&mut vfs).unwrap();
    let mut n = 0;
    for rec in idx.records() {
        if !rec.resource.to_lowercase().contains(&pat) { continue; }
        let p = rec.position();
        if (p[0]-cx).abs() <= r && (p[2]-cz).abs() <= r {
            n += 1;
            if n <= 60 { println!("{} {:?} in {}", rec.resource, p, rec.document.rsplit('/').next().unwrap()); }
        }
    }
    println!("{n} matches");
}
