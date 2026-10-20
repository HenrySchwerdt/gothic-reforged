use gothic_formats::{vfs::VirtualFileSystem, terrain::vegetation, xcmsh::Xcmsh};
use std::collections::HashMap;
fn main() {
    let mut vfs = VirtualFileSystem::open_data_dir(std::path::Path::new(r"D:\SteamLibrary\steamapps\common\Gothic 3\Data")).unwrap();
    // terrain heights bucketed at 500 cm
    let names: Vec<String> = vfs.iter().filter(|(p, _)| { let l = p.to_lowercase(); l.ends_with(".xcmsh") && l.contains("_landscape_cell_") && !l.contains("lowpoly") }).map(|(p, _)| p.to_string()).collect();
    println!("{} cells", names.len());
    let mut grid: HashMap<(i32, i32), Vec<[f32; 3]>> = HashMap::new();
    for n in &names {
        let slot = vfs.resolve(n).unwrap();
        let Ok(m) = Xcmsh::parse(&vfs.read(slot).unwrap()) else { continue };
        for s in &m.submeshes { for p in &s.positions { grid.entry(((p[0] / 500.0).floor() as i32, (p[2] / 500.0).floor() as i32)).or_default().push(*p); } }
    }
    let height = |x: f32, z: f32| -> Option<f32> {
        let (cx, cz) = ((x / 500.0).floor() as i32, (z / 500.0).floor() as i32);
        let mut best: Option<(f32, f32)> = None;
        for dx in -1..=1 { for dz in -1..=1 { if let Some(v) = grid.get(&(cx + dx, cz + dz)) { for p in v { let d = (p[0]-x).powi(2) + (p[2]-z).powi(2); if best.is_none_or(|b| d < b.0) { best = Some((d, p[1])); } } } } }
        best.filter(|b| b.0 < 300.0f32.powi(2)).map(|b| b.1)
    };
    let docs: Vec<String> = vfs.iter().filter(|(p, _)| p.to_lowercase().ends_with("_cstat.node")).map(|(p, _)| p.to_string()).collect();
    let mut n_areas = 0;
    for d in &docs {
        let slot = vfs.resolve(d).unwrap();
        let v = vegetation::extract(&vfs.read(slot).unwrap()).unwrap();
        for ar in &v.areas {
            if n_areas >= 6 { return; }
            n_areas += 1;
            let sec = d.rsplit('/').next().unwrap();
            let m = &ar.meshes[0];
            println!("{sec} area {} matrix t {:?}", ar.entity, ar.matrix[3]);
            // sector centre from name _x95000y0z-15000
            let at = sec.rfind("_x").unwrap();
            let rest = &sec[at+2..]; let (sx, rest) = rest.split_once('y').unwrap(); let (_, rest) = rest.split_once('z').unwrap();
            let sz: String = rest.chars().take_while(|c| *c == '-' || c.is_ascii_digit()).collect();
            let (sx, sz): (f32, f32) = (sx.parse().unwrap(), sz.parse().unwrap());
            for (label, scale, ox, oz) in [("sector+v*100", 100.0, sx, sz), ("sector+v*10", 10.0, sx, sz), ("sector+v", 1.0, sx, sz)] {
                let mut xs = vec![]; let mut ys = vec![];
                for p in m.positions.iter() {
                    if let Some(h) = height(ox + p[0] * scale, oz + p[2] * scale) { xs.push(p[1] * scale); ys.push(h); }
                }
                if xs.len() < 5 { println!("  {label}: no terrain ({})", xs.len()); continue; }
                let n = xs.len() as f32; let mx = xs.iter().sum::<f32>() / n; let my = ys.iter().sum::<f32>() / n;
                let sxy: f32 = xs.iter().zip(&ys).map(|(a, b)| (a - mx) * (b - my)).sum();
                let sxx: f32 = xs.iter().map(|a| (a - mx).powi(2)).sum(); let syy: f32 = ys.iter().map(|b| (b - my).powi(2)).sum();
                println!("  {label}: n={} slope={:.2} r={:.3} (vy range {:.0}, ty range {:.0})", xs.len(), sxy / sxx.max(1e-6), sxy / (sxx * syy).sqrt().max(1e-6), sxx.sqrt(), syy.sqrt());
            }
        }
    }
}
