use gothic_formats::{vfs::VirtualFileSystem, xcmsh::Xcmsh};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut vfs = VirtualFileSystem::open_data_dir(std::path::Path::new(r"D:\SteamLibrary\steamapps\common\Gothic 3\Data")).unwrap();
    let slot = vfs.resolve_name(&a[1]).expect("not found");
    let m = Xcmsh::parse(&vfs.read(slot).unwrap()).unwrap();
    for (i, s) in m.submeshes.iter().enumerate() {
        println!("sub {i}: mat={} v={} tri={} c4={} c5={} tan={} uv2={} uv={}", s.material, s.positions.len(), s.indices.len()/3, s.colors4.len(), s.colors5.len(), s.tangents.len(), s.uv2.len(), s.uvs.len());
        let n = a.get(2).and_then(|x| x.parse().ok()).unwrap_or(6usize);
        for k in 0..n.min(s.positions.len()) {
            let k = k * (s.positions.len() / n.max(1)).max(1);
            if k >= s.positions.len() { break; }
            println!("   v{k} pos={:?} uv={:?} c4={:?} c5={:?} uv2={:?} tan={:?}", s.positions[k], s.uvs.get(k), s.colors4.get(k), s.colors5.get(k), s.uv2.get(k), s.tangents.get(k));
        }
    }
}
