use gothic_formats::vfs::VirtualFileSystem;
use std::path::Path;
fn main() -> anyhow::Result<()> {
    let mut vfs = VirtualFileSystem::open_data_dir(Path::new(r"D:\SteamLibrary\steamapps\common\Gothic 3\Data"))?;
    let name = std::env::args().nth(1).unwrap();
    let paths: Vec<_> = vfs.iter().filter(|(p,_)| p.contains(&name.to_lowercase())).map(|(p,s)|(p.to_string(),s)).collect();
    for (p, s) in &paths {
        println!("{p} {}", vfs.archive_name(*s));
    }
    if let Some((p, s)) = paths.first() {
        let b = vfs.read(*s)?;
        std::fs::write(format!(".local/scratch/{}", p.rsplit('/').next().unwrap()), &b)?;
        println!("{} bytes", b.len());
    }
    Ok(())
}
