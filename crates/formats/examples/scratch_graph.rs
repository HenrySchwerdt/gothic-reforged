use gothic_formats::{vfs::VirtualFileSystem, xshmat::graph::{MaterialGraph, Node}};
use std::collections::BTreeMap;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut vfs = VirtualFileSystem::open_data_dir(std::path::Path::new(r"D:\SteamLibrary\steamapps\common\Gothic 3\Data")).unwrap();
    if a.len() > 1 && a[1] != "--survey" {
        for n in &a[1..] {
            let slot = vfs.resolve_name(n).expect("nf");
            let g = MaterialGraph::parse(&vfs.read(slot).unwrap()).unwrap();
            println!("{n}: slots {:?}", g.slots);
            for (i, nd) in g.nodes.iter().enumerate() { println!("  #{i} {nd:?}"); }
            println!("  recipe {:#?}", g.surface());
        }
        return;
    }
    let names: Vec<String> = vfs.iter().filter(|(p, _)| p.to_lowercase().ends_with(".xshmat")).map(|(p, _)| p.to_string()).collect();
    let mut fails = 0; let mut kinds: BTreeMap<String, usize> = BTreeMap::new(); let mut comb: BTreeMap<u32, usize> = BTreeMap::new();
    let mut multi = 0; let mut vblend = 0; let mut noslot = 0; let mut norecipe = 0; let mut nlayers: BTreeMap<usize, usize> = BTreeMap::new();
    for n in &names {
        let slot = vfs.resolve(n).unwrap();
        match MaterialGraph::parse(&vfs.read(slot).unwrap()) {
            Ok(g) => {
                for nd in &g.nodes {
                    let k = match nd { Node::Other(s) => format!("Other:{s}"), x => format!("{:?}", x).split(|c: char| !c.is_alphanumeric()).next().unwrap().to_string() };
                    *kinds.entry(k).or_default() += 1;
                    if let Node::Combiner { kind, .. } = nd { *comb.entry(*kind).or_default() += 1; }
                }
                if g.slots.is_empty() { noslot += 1; }
                match g.surface() { Some(r) => { *nlayers.entry(r.layers.len()).or_default() += 1; if r.vertex_blend { vblend += 1; } if r.layers.iter().any(|l| l.textures.len() > 2) { multi += 1; } } None => norecipe += 1 }
            }
            Err(e) => { fails += 1; if fails < 5 { println!("fail {n}: {e:#}"); } }
        }
    }
    println!("{} materials, fails {fails}, noslot {noslot}, norecipe {norecipe}, vblend {vblend}, layers>2tex {multi}", names.len());
    println!("{kinds:?}
combiner kinds {comb:?}
layers {nlayers:?}");
}
