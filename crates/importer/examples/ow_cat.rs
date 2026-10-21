//! `ow_cat DATA_DIR path-suffix`: write an archive entry to stdout (development tool).
use anyhow::{Context, Result};
use gothic_formats::vfs::VirtualFileSystem;
use std::{io::Write, path::Path};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut vfs = VirtualFileSystem::open_data_dir(Path::new(&args[0]))?;
    let suffix = args[1].to_ascii_lowercase();
    let slot = vfs
        .iter()
        .find(|(p, _)| p.to_ascii_lowercase().ends_with(&suffix))
        .map(|(_, s)| s)
        .context("not found")?;
    std::io::stdout().write_all(&vfs.read(slot)?)?;
    Ok(())
}
