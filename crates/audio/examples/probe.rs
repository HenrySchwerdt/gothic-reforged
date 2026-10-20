use gothic_formats::vfs::VirtualFileSystem;
use gothic_gameplay::GameDatabase;
use std::path::Path;
fn main() {
    let d = Path::new("D:/SteamLibrary/steamapps/common/Gothic 3/Data");
    let db = GameDatabase::load_install(d, "German").unwrap();
    let mut n = 0;
    for e in &db.strings.entries { if e.key.starts_with("INFO_") || e.key.starts_with("SVM_") { n+=1; if n % 900 == 0 { println!("KEY {}", e.key); } } }
    let vfs = VirtualFileSystem::open_data_dir(d).unwrap();
    let mut v: Vec<&str> = vfs.iter().map(|x| x.0).filter(|p| p.starts_with("german_ogg/")).collect();
    v.sort();
    for (i, p) in v.iter().enumerate() { if i % 1200 == 0 { println!("FILE {p}"); } }
    // sample infos
    let name = db.owners().next().unwrap().to_string();
    for id in db.infos_of(&name).iter().take(2) { println!("{:?}", db.info(*id).script.iter().filter(|s| !s.text.is_empty()).take(3).collect::<Vec<_>>()); }
}
