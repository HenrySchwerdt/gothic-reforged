//! `ow_index DATA_DIR`: build the spatial index and print record / failed-document counts.
use anyhow::Result;
use gothic_formats::vfs::VirtualFileSystem;
use gothic_importer::world_index::WorldIndex;
use std::path::Path;

fn main() -> Result<()> {
    let dir = std::env::args().nth(1).unwrap_or_default();
    let mut vfs = VirtualFileSystem::open_data_dir(Path::new(&dir))?;
    let index = WorldIndex::build(&mut vfs)?;
    println!(
        "records {} failed {}",
        index.len(),
        index.failed_documents.len()
    );
    Ok(())
}
