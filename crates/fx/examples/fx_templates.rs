use gothic_formats::vfs::VirtualFileSystem;
use gothic_fx::efm::{CommandKind, EffectMap};
use gothic_fx::templates::FxTemplates;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let data = std::path::Path::new(&a[1]);
    let mut vfs = VirtualFileSystem::open_data_dir(data).unwrap();
    let t = std::time::Instant::now();
    let tpl = FxTemplates::load(&mut vfs).unwrap();
    println!("{} particle templates in {:?}", tpl.len(), t.elapsed());
    let map = EffectMap::parse(&std::fs::read(&a[2]).unwrap()).unwrap();
    let (mut spawns, mut hit, mut miss) = (0, 0, 0);
    for n in map.names() {
        for c in &map.lookup(n).unwrap().commands {
            if let CommandKind::SpawnEntity {
                template: Some(g), ..
            } = &c.kind
            {
                spawns += 1;
                if tpl.by_guid(g).is_some() {
                    hit += 1
                } else {
                    miss += 1
                }
            }
        }
    }
    println!("spawn commands {spawns}: resolved {hit}, unresolved {miss}");
    for n in &a[3..] {
        let seq = map.lookup(n).unwrap();
        for c in &seq.commands {
            if let CommandKind::SpawnEntity {
                template: Some(g),
                link,
                auto_kill,
                offset,
                ..
            } = &c.kind
            {
                match tpl.by_guid(g) {
                    Some(s) => println!(
                        "{n} t={} {link:?} kill={auto_kill} off={offset:?} -> {} ({} emitters): {:?}",
                        c.time,
                        s.name,
                        s.emitters.len(),
                        s.emitters
                            .iter()
                            .map(|e| (
                                e.name.clone(),
                                e.params.texture.clone(),
                                e.local.w_axis.truncate()
                            ))
                            .collect::<Vec<_>>()
                    ),
                    None => println!("{n} t={} -> unresolved {g}", c.time),
                }
            }
        }
    }
}
