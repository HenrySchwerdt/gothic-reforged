use gothic_formats::{vfs::VirtualFileSystem, xshmat::Xshmat};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut vfs = VirtualFileSystem::open_data_dir(std::path::Path::new(r"D:\SteamLibrary\steamapps\common\Gothic 3\Data")).unwrap();
    for n in &a[1..] {
        let slot = vfs.resolve_name(n).expect("not found");
        let bytes = vfs.read(slot).unwrap();
        let m = Xshmat::parse(&bytes).unwrap();
        println!("{n} ({} bytes): blend={:?} mask={:?} spec={:?} nolight={:?}", bytes.len(), m.blend_mode, m.mask_reference, m.enable_specular, m.disable_lighting);
        for t in &m.textures { println!("    {:?} {}", t.role, t.path); }
    }
}
