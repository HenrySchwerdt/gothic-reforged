//! `ow_extract DATA_DIR OUT_DIR [path-substring ...]`: write the world documents that do not
//! parse (or match a substring) to OUT_DIR, named by their basename (development tool).
use anyhow::Result;
use gothic_formats::vfs::VirtualFileSystem;
use gothic_formats::world_node::WorldDocument;
use std::path::Path;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut vfs = VirtualFileSystem::open_data_dir(Path::new(&args[0]))?;
    let out = Path::new(&args[1]);
    std::fs::create_dir_all(out)?;
    let subs = &args[2..];
    let slots: Vec<_> = vfs
        .iter()
        .filter(|(p, _)| {
            let l = p.to_ascii_lowercase();
            l.ends_with(".node")
                || l.ends_with(".lrentdat")
                || l.ends_with(".secdat")
                || l.ends_with(".sec")
                || l.ends_with(".lrgeo")
                || l.ends_with(".lrgeodat")
        })
        .map(|(p, s)| (p.to_owned(), s))
        .collect();
    for (path, slot) in slots {
        let bytes = vfs.read(slot)?;
        let pick = if subs.is_empty() {
            bytes.len() != 10 && WorldDocument::parse(&bytes).is_err()
        } else {
            subs.iter().any(|s| path.to_ascii_lowercase().contains(s))
        };
        if pick {
            let name = path.rsplit('/').next().unwrap_or(&path);
            std::fs::write(out.join(name), &bytes)?;
            println!("{} {}", bytes.len(), path);
        }
    }
    Ok(())
}
