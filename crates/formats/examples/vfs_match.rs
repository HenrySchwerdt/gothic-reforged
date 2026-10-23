//! Compare one resolved VFS member with an existing reference file.
//! Usage: vfs_match <Data dir> <member basename> <reference file>
use anyhow::{Context, Result, bail};
use gothic_formats::{vfs::VirtualFileSystem, ximg::Ximg};
use std::path::Path;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        bail!("usage: vfs_match <Data dir> <member basename> <reference file>");
    }

    let mut vfs = VirtualFileSystem::open_data_dir(Path::new(&args[1]))
        .with_context(|| format!("open VFS data directory {}", args[1]))?;
    let slot = vfs
        .resolve_name(&args[2])
        .with_context(|| format!("VFS member not found: {}", args[2]))?;
    let payload = vfs.read(slot).context("read resolved VFS member")?;
    let reference_path = Path::new(&args[3]);
    let reference = std::fs::read(reference_path)
        .with_context(|| format!("read reference file {}", reference_path.display()))?;
    let compared = if reference_path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("dds"))
    {
        Ximg::parse(&payload)
            .context("reference has .dds extension; resolved VFS payload must be XIMG")?
            .to_dds()
    } else {
        payload.clone()
    };
    let matched = compared == reference;

    println!("archive_name={}", vfs.archive_name(slot));
    println!("entry_path={}", vfs.entry_path(slot));
    println!("payload_len={}", payload.len());
    println!("reference_len={}", reference.len());
    println!("MATCH {matched}");
    if !matched {
        bail!("resolved VFS payload does not match reference file");
    }
    Ok(())
}
