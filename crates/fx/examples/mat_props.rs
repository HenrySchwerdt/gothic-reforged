//! Survey of texture-coordinate animation in the installed materials.
use gothic_formats::vfs::VirtualFileSystem;
use gothic_formats::xshmat::graph::MaterialGraph;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut vfs = VirtualFileSystem::open_data_dir(std::path::Path::new(&a[1])).unwrap();
    let slots: Vec<_> = vfs
        .iter()
        .filter(|(p, _)| p.to_ascii_lowercase().ends_with(".xshmat"))
        .map(|(p, s)| (p.to_owned(), s))
        .collect();
    let (mut total, mut anim, mut scroll, mut osc, mut rot) = (0, 0, 0, 0, 0);
    for (p, s) in &slots {
        let b = vfs.read(*s).unwrap();
        let Ok(g) = MaterialGraph::parse(&b) else {
            continue;
        };
        total += 1;
        let mut any = false;
        for n in 0..g.nodes.len() {
            for t in g.samplers(n) {
                if !t.anim.is_empty() {
                    any = true;
                    if t.anim.scroll.is_some() {
                        scroll += 1;
                    }
                    if t.anim.oscillate.is_some() {
                        osc += 1;
                    }
                    if t.anim.rotate.is_some() {
                        rot += 1;
                    }
                    if a.len() > 2 {
                        println!("{p}: {} {:?}", t.path, t.anim);
                    }
                    break;
                }
            }
        }
        if any {
            anim += 1;
        }
    }
    println!("{total} graphs, {anim} animated (scroll {scroll}, oscillate {osc}, rotate {rot})");
}
