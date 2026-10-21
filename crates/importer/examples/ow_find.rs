//! `ow_find DATA_DIR substring`: every entry of every archive whose path contains the substring.
use anyhow::Result;
use gothic_formats::PakArchive;
use std::path::Path;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let needle = args[1].to_ascii_lowercase();
    for e in std::fs::read_dir(Path::new(&args[0]))? {
        let p = e?.path();
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !(name.ends_with(".pak") || name.contains(".p0") || name.contains(".p1")) {
            continue;
        }
        let Ok(a) = PakArchive::open(&p) else {
            continue;
        };
        for en in &a.entries {
            if en.path.to_ascii_lowercase().contains(&needle) {
                println!("{name}: {}", en.path);
            }
        }
    }
    Ok(())
}
