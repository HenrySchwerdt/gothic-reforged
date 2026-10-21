//! `weather_dump DATA_DIR`: decode WeatherStates.g3ws and print a summary (development tool).
use anyhow::{Context, Result};
use gothic_formats::vfs::VirtualFileSystem;
use std::path::Path;

fn main() -> Result<()> {
    let dir = std::env::args().nth(1).context("DATA_DIR")?;
    let mut vfs = VirtualFileSystem::open_data_dir(Path::new(&dir))?;
    let slot = vfs
        .iter()
        .find(|(p, _)| p.to_ascii_lowercase().ends_with("weatherstates.g3ws"))
        .map(|(p, s)| (p.to_owned(), s))
        .context("g3ws not found")?;
    println!("{}", slot.0);
    let bytes = vfs.read(slot.1)?;
    let w = gothic_formats::weather::parse(&bytes)?;
    for s in &w.states {
        println!("{} sun@0.5 {:?} int {} fog {}..{}", s.name, s.scales.sun_diffuse.at(0.5), s.scales.sun_intensity.at(0.5), s.scales.fog_start.at(0.5), s.scales.fog_end.at(0.5));
    }
    for e in &w.environments { println!("{} {} entries", e.name, e.entries.len()); }
    Ok(())
}
