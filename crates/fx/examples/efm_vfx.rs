use gothic_formats::vfs::VirtualFileSystem;
use gothic_fx::efm::{CommandKind, EffectMap};
use gothic_fx::templates::FxTemplates;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut vfs = VirtualFileSystem::open_data_dir(std::path::Path::new(&a[1])).unwrap();
    let tpl = FxTemplates::load(&mut vfs).unwrap();
    let map = EffectMap::parse(&std::fs::read(&a[2]).unwrap()).unwrap();
    let mut names: Vec<&str> = map.names().collect();
    names.sort();
    for n in names {
        let seq = map.lookup(n).unwrap();
        let sets: Vec<String> = seq
            .commands
            .iter()
            .filter_map(|c| match &c.kind {
                CommandKind::SpawnEntity {
                    template: Some(g), ..
                } => tpl.by_guid(g).map(|s| s.name.clone()),
                _ => None,
            })
            .collect();
        if !sets.is_empty() && a.get(3).is_none_or(|f| n.contains(f.as_str())) {
            println!("{n}: {}", sets.join(", "));
        }
    }
}
