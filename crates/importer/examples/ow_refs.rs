//! `ow_refs DATA_DIR`: which `.lrentdat` / `.node` layers are named by some `.secdat` string table
//! (the sector's dynamic and geometry layer lists), and which have a `.lrent` / `.lrgeo` header
//! file. Prints the unreferenced ones (development tool, counts and paths only).
use anyhow::Result;
use gothic_formats::genome::GenomeFile;
use gothic_formats::vfs::VirtualFileSystem;
use std::collections::BTreeSet;
use std::path::Path;

fn stem(p: &str) -> String {
    let f = p.rsplit('/').next().unwrap_or(p);
    f.rsplit_once('.').map_or(f, |x| x.0).to_ascii_lowercase()
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut vfs = VirtualFileSystem::open_data_dir(Path::new(&args[0]))?;
    let all: Vec<(String, _)> = vfs.iter().map(|(p, s)| (p.to_owned(), s)).collect();
    let paths: BTreeSet<String> = all.iter().map(|(p, _)| p.clone()).collect();
    let mut named = BTreeSet::new();
    for (p, s) in &all {
        if p.ends_with(".secdat") {
            let bytes = vfs.read(*s)?;
            if let Ok(g) = GenomeFile::parse(&bytes) {
                for s in g.strings {
                    named.insert(String::from_utf8_lossy(s).to_ascii_lowercase());
                }
            } else if bytes.len() >= 10 && !bytes.starts_with(b"GENOMFLE") {
                // Raw sector: u16 version, u32 dynamic layer count, u32 geometry layer count,
                // then that many inline `u16 length + bytes` names.
                let n = u32::from_le_bytes(bytes[2..6].try_into()?) as usize
                    + u32::from_le_bytes(bytes[6..10].try_into()?) as usize;
                let mut i = 10;
                for _ in 0..n.min(4096) {
                    let Some(l) = bytes.get(i..i + 2) else { break };
                    let l = u16::from_le_bytes([l[0], l[1]]) as usize;
                    let Some(name) = bytes.get(i + 2..i + 2 + l) else { break };
                    named.insert(String::from_utf8_lossy(name).to_ascii_lowercase());
                    i += 2 + l;
                }
            }
        }
    }
    let (mut total, mut unref) = (0, 0);
    for (p, _) in &all {
        let (header, is_layer) = if p.ends_with(".lrentdat") {
            (p.replace(".lrentdat", ".lrent"), true)
        } else if p.ends_with(".lrgeodat") {
            (p.replace(".lrgeodat", ".lrgeo"), true)
        } else {
            (String::new(), false)
        };
        if !is_layer {
            continue;
        }
        total += 1;
        let referenced = named.contains(&stem(p));
        let has_header = paths.contains(&header);
        if !referenced || !has_header {
            unref += 1;
            println!("{} referenced={referenced} header={has_header} {p}", if referenced { "HDR?" } else { "ORPH" });
        }
    }
    println!("layers {total}, not referenced or no header {unref}");
    Ok(())
}
