//! `cargo run -p gothic-importer --example survey -- world|mesh DATA_DIR OUT.json`
use anyhow::{Context, Result};
use gothic_formats::vfs::VirtualFileSystem;
use std::path::Path;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mode, dir, out) = (
        args.first().context("mode")?,
        args.get(1).context("DATA_DIR")?,
        args.get(2).context("OUT.json")?,
    );
    let mut vfs = VirtualFileSystem::open_data_dir(Path::new(dir))?;
    let json = match mode.as_str() {
        "world" => serde_json::to_vec_pretty(&gothic_importer::survey::world_survey(&mut vfs)?)?,
        "mesh" => serde_json::to_vec_pretty(&gothic_importer::survey::mesh_survey(&mut vfs)?)?,
        other => anyhow::bail!("unknown mode {other}"),
    };
    std::fs::write(out, json)?;
    Ok(())
}
