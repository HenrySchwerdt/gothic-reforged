//! `veg_grid DATA_DIR [doc substring]`: statistics of the decoded vegetation grids (development tool).
use anyhow::Result;
use gothic_formats::terrain::vegetation;
use gothic_formats::vfs::VirtualFileSystem;
use std::path::Path;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut vfs = VirtualFileSystem::open_data_dir(Path::new(&args[0]))?;
    let filter = args.get(1).map(|s| s.to_ascii_lowercase());
    let slots: Vec<_> = vfs
        .iter()
        .filter(|(p, _)| {
            let l = p.to_ascii_lowercase();
            (l.ends_with(".node") || l.ends_with(".lrentdat"))
                && filter.as_ref().is_none_or(|f| l.contains(f))
        })
        .map(|(p, s)| (p.to_owned(), s))
        .collect();
    let (mut areas, mut with_grid, mut total) = (0, 0, 0usize);
    for (path, slot) in slots {
        let Ok(bytes) = vfs.read(slot) else { continue };
        let Ok(v) = vegetation::extract(&bytes) else {
            continue;
        };
        for a in &v.areas {
            areas += 1;
            let Some(g) = &a.grid else { continue };
            with_grid += 1;
            total += g.entries.len();
            if with_grid <= 6 {
                println!(
                    "{path} {} meshes={} nodes={} entries={} node={} view={} fade={} ent_pos=({:.0},{:.0},{:.0})",
                    a.entity,
                    a.meshes.len(),
                    g.nodes,
                    g.entries.len(),
                    g.node_size,
                    a.view_range,
                    a.fade_out_start,
                    a.matrix[3][0],
                    a.matrix[3][1],
                    a.matrix[3][2]
                );
                for m in &a.meshes {
                    println!("   mesh {} {} spacing {}", m.mesh_id, m.mesh, m.min_spacing);
                }
                for e in g.entries.iter().take(4) {
                    println!("   {e:?}");
                }
            }
        }
    }
    println!("areas {areas} with grid {with_grid} total entries {total}");
    Ok(())
}
