//! Write the synthetic seed corpora for the fuzz targets:
//! `cargo run -p gothic-formats --example fuzz_seeds -- crates/formats/fuzz/corpus`.
use gothic_formats::synth::{Target, seeds};
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "crates/formats/fuzz/corpus".into()),
    );
    for t in Target::ALL {
        let dir = root.join(t.name());
        std::fs::create_dir_all(&dir)?;
        for (i, s) in seeds(t).iter().enumerate() {
            std::fs::write(dir.join(format!("seed_{i:02}")), s)?;
        }
    }
    println!("seeds written to {}", root.display());
    Ok(())
}
