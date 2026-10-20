use gothic_formats::vfs::VirtualFileSystem;
use std::collections::BTreeMap;
use std::path::Path;
fn main() {
    let vfs = VirtualFileSystem::open_data_dir(Path::new("D:/SteamLibrary/steamapps/common/Gothic 3/Data")).unwrap();
    let mut m: BTreeMap<String, (usize, String)> = BTreeMap::new();
    let mut all: Vec<(String, String)> = vec![];
    for (p, s) in vfs.iter() {
        let l = p.to_string();
        if l.ends_with(".ogg") || l.ends_with(".wav") || l.ends_with(".xlip") || l.contains("sound") || l.contains("music") || l.contains("speech") {
            let a = vfs.archive_name(s).to_string();
            let dir = l.rsplit_once('/').map(|x| x.0).unwrap_or("").to_string();
            let ext = l.rsplit('.').next().unwrap().to_string();
            let e = m.entry(format!("{a} | {dir} | .{ext}")).or_insert((0, l.clone()));
            e.0 += 1;
            all.push((l, a));
        }
    }
    for (k, (n, ex)) in &m { println!("{n:6} {k}   e.g. {ex}"); }
}
