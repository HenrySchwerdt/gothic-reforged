//! `ow_try FILE...`: parse each world document and print entity/edge counts or the error.
use anyhow::Result;
use gothic_formats::world_node::WorldDocument;

fn main() -> Result<()> {
    for path in std::env::args().skip(1) {
        let bytes = std::fs::read(&path)?;
        match WorldDocument::parse(&bytes) {
            Ok(d) => println!(
                "OK   entities={} edges={} with_assets={} bytes={} {}",
                d.entities.len(),
                d.edges.len(),
                d.entities.iter().filter(|e| !e.asset_references.is_empty()).count(),
                bytes.len(),
                path
            ),
            Err(e) => println!("FAIL {} {}: {e:#}", bytes.len(), path),
        }
    }
    Ok(())
}
