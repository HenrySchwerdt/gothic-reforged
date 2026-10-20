use gothic_formats::{vfs::VirtualFileSystem, terrain::{vegetation, spt::Spt}};
use std::collections::BTreeMap;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut vfs = VirtualFileSystem::open_data_dir(std::path::Path::new(r"D:\SteamLibrary\steamapps\common\Gothic 3\Data")).unwrap();
    if a.get(1).map(String::as_str) == Some("spt") {
        let names: Vec<String> = vfs.iter().filter(|(p, _)| p.to_lowercase().ends_with(".spt")).map(|(p, _)| p.to_string()).collect();
        for n in names {
            let slot = vfs.resolve(&n).unwrap();
            match Spt::parse(&vfs.read(slot).unwrap()) { Ok(s) => println!("{n}: size {} bark {} tex {:?}", s.size, s.bark, s.textures), Err(e) => println!("{n}: ERR {e:#}") }
        }
        return;
    }
    let docs: Vec<String> = vfs.iter().filter(|(p, _)| p.to_lowercase().ends_with("_cstat.node")).map(|(p, _)| p.to_string()).collect();
    let (mut trees, mut areas, mut meshes, mut rej, mut fail) = (0, 0, 0, 0, 0);
    let mut spts: BTreeMap<String, usize> = BTreeMap::new();
    let mut brush: BTreeMap<String, usize> = BTreeMap::new();
    let mut tris = 0usize;
    for d in &docs {
        let slot = vfs.resolve(d).unwrap();
        match vegetation::extract(&vfs.read(slot).unwrap()) {
            Ok(v) => { trees += v.trees.len(); areas += v.areas.len(); rej += v.rejected_meshes;
                for t in &v.trees { *spts.entry(t.spt.clone()).or_default() += 1; }
                for ar in &v.areas { meshes += ar.meshes.len(); for m in &ar.meshes { *brush.entry(m.mesh.clone()).or_default() += 1; tris += m.indices.len()/3; } }
                if a.len() > 1 && d.contains(&a[1]) { println!("{d}: trees {} areas {}", v.trees.len(), v.areas.len()); for t in v.trees.iter().take(3) { println!("  tree {} {} {:?}", t.entity, t.spt, t.matrix); } for ar in &v.areas { println!("  area {} matrix {:?} view {} fade {} grid {}", ar.entity, ar.matrix, ar.view_range, ar.fade_out_start, ar.grid_node_size); for m in &ar.meshes { println!("    {} spacing {} wind {} dbl {} shade {} v {} tri {} bb {:?}..{:?}", m.mesh, m.min_spacing, m.wind_strength, m.double_sided, m.shading, m.positions.len(), m.indices.len()/3, m.bounds_min, m.bounds_max); } } }
            }
            Err(e) => { fail += 1; if fail < 4 { println!("fail {d}: {e:#}"); } }
        }
    }
    println!("{} docs fail {fail} trees {trees} areas {areas} meshes {meshes} rejected {rej} tris {tris}", docs.len());
    println!("spt kinds {}: {:?}", spts.len(), spts.iter().take(12).collect::<Vec<_>>());
    println!("brush kinds {}: {:?}", brush.len(), brush);
}
