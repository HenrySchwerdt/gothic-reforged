mod actors;
mod gamedata;
mod objdump;
mod propdump;
use anyhow::{Context, Result, bail, ensure};
use gothic_formats::{PakArchive, normalize_path};
use std::{
    io::Write,
    path::{Path, PathBuf},
};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let command = args.first().and_then(|s| s.to_str()).unwrap_or("help");
    match command {
        "scene-info" => {
            ensure!(
                args.len() == 4,
                "scene-info requires WORLD_ARCHIVE WORLD_MEMBER MESH_ARCHIVE"
            );
            let mut archive = PakArchive::open(&args[1])?;
            let member = normalize_path(args[2].to_str().context("member must be Unicode")?)?;
            let index = archive
                .entries
                .iter()
                .position(|e| e.path.eq_ignore_ascii_case(&member))
                .context("world member not found")?;
            let world =
                gothic_formats::world_node::WorldDocument::parse(&archive.read_payload(index)?)?;
            let scene =
                gothic_importer::preview_scene::import_preview(&world, Path::new(&args[3]))?;
            serde_json::to_writer_pretty(std::io::stdout().lock(), &scene)?;
        }
        "world-survey" | "mesh-survey" | "world-query" => {
            ensure!(args.len() >= 2, "{command} requires DATA_DIR");
            let mut vfs =
                gothic_formats::vfs::VirtualFileSystem::open_data_dir(Path::new(&args[1]))?;
            eprintln!(
                "mounted {} archives, {} visible entries, {} overridden, {} payloadless ignored, {} skipped",
                vfs.archive_names().len(),
                vfs.len(),
                vfs.overridden,
                vfs.ignored_payloadless.len(),
                vfs.skipped.len()
            );
            let json = match command {
                "world-survey" => {
                    serde_json::to_value(gothic_importer::survey::world_survey(&mut vfs)?)?
                }
                "mesh-survey" => {
                    serde_json::to_value(gothic_importer::survey::mesh_survey(&mut vfs)?)?
                }
                _ => {
                    ensure!(
                        args.len() == 6,
                        "world-query requires DATA_DIR X Y Z RADIUS"
                    );
                    let num = |i: usize| -> Result<f32> {
                        Ok(args[i].to_str().context("number")?.parse()?)
                    };
                    let index = gothic_importer::world_index::WorldIndex::build(&mut vfs)?;
                    eprintln!(
                        "indexed {} placements from {} documents",
                        index.len(),
                        index.documents
                    );
                    serde_json::to_value(index.query([num(2)?, num(3)?, num(4)?], num(5)?))?
                }
            };
            if let Some(out) = args.get(2).filter(|_| command != "world-query") {
                std::fs::write(out, serde_json::to_vec_pretty(&json)?)?;
            } else {
                serde_json::to_writer_pretty(std::io::stdout().lock(), &json)?;
            }
        }
        "scan" => {
            let root = args
                .get(1)
                .context("scan requires installation directory")?;
            let report = gothic_importer::scan(Path::new(root))?;
            serde_json::to_writer_pretty(std::io::stdout().lock(), &report)?;
        }
        "list" => {
            let path = args.get(1).context("list requires archive")?;
            let pak = PakArchive::open(path)?;
            let filter = args
                .get(2)
                .map(|s| s.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let entries: Vec<_> = pak
                .entries
                .iter()
                .filter(|e| e.path.to_lowercase().contains(&filter))
                .collect();
            serde_json::to_writer_pretty(std::io::stdout().lock(), &entries)?;
        }
        "mesh-info" | "world-info" => {
            ensure!(
                args.len() == 3,
                "mesh-info/world-info requires ARCHIVE MEMBER"
            );
            let mut pak = PakArchive::open(&args[1])?;
            let member = normalize_path(args[2].to_str().context("member must be Unicode")?)?;
            let index = pak
                .entries
                .iter()
                .position(|e| e.path.eq_ignore_ascii_case(&member))
                .context("member not found")?;
            let bytes = pak.read_payload(index)?;
            if command == "mesh-info" {
                let mesh = gothic_formats::xcmsh::Xcmsh::parse(&bytes)?;
                serde_json::to_writer_pretty(std::io::stdout().lock(), &mesh)?;
            } else {
                let world = gothic_formats::world_node::WorldDocument::parse(&bytes)?;
                serde_json::to_writer_pretty(std::io::stdout().lock(), &world)?;
            }
        }
        "extract" | "ximg-dds" => {
            ensure!(
                args.len() == 4,
                "extract/ximg-dds requires ARCHIVE MEMBER OUTPUT_FILE"
            );
            let mut pak = PakArchive::open(&args[1])?;
            let member = normalize_path(args[2].to_str().context("member must be Unicode")?)?;
            let index = pak
                .entries
                .iter()
                .position(|e| e.path.eq_ignore_ascii_case(&member))
                .context("member not found")?;
            let output = PathBuf::from(&args[3]);
            // Output is an explicit caller-selected file; never join an untrusted member path.
            let parent = output
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            let parent = parent
                .canonicalize()
                .context("output parent must already exist")?;
            let archive = Path::new(&args[1]).canonicalize()?;
            // Refuse writes anywhere beneath an installation root, including symlinked parents.
            for ancestor in archive.ancestors().skip(1) {
                if ancestor.join("Gothic3.exe").is_file() && parent.starts_with(ancestor) {
                    bail!("output must be outside original installation");
                }
            }
            let payload = pak.read_payload(index)?;
            let payload = if command == "ximg-dds" {
                gothic_formats::ximg::Ximg::parse(&payload)?.to_dds()
            } else {
                payload
            };
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&output)
                .context("output must not already exist")?;
            file.write_all(&payload)?;
            eprintln!("Wrote {} bytes to {}", payload.len(), output.display());
        }
        "help" | "--help" | "-h" => println!(
            "gothic-inspector scan INSTALLATION\ngothic-inspector list ARCHIVE [FILTER]\ngothic-inspector extract ARCHIVE MEMBER OUTPUT_FILE\ngothic-inspector ximg-dds ARCHIVE MEMBER OUTPUT_FILE\ngothic-inspector mesh-info ARCHIVE MEMBER\ngothic-inspector world-info ARCHIVE MEMBER\ngothic-inspector scene-info WORLD_ARCHIVE WORLD_MEMBER MESH_ARCHIVE\nRead-only source access. Extraction creates one new file; no overwrite."
        ),
        other => {
            if !actors::dispatch(other, &args)?
                && !gamedata::dispatch(other, &args)?
                && !propdump::dispatch(other, &args)?
                && !objdump::dispatch(other, &args)?
                && !gothic_cook::cli::dispatch(other, &args)?
            {
                bail!("unknown command {other}; use --help")
            }
        }
    }
    Ok(())
}
