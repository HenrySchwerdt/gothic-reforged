//! Debug: list the effects that create decals. Usage: efm_decals <Data dir>
use gothic_formats::{
    genome::GenomeFile,
    propstream::{Cursor, DecodeStats, Strings, read_sub_class},
    vfs::VirtualFileSystem,
};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut vfs = VirtualFileSystem::open_data_dir(std::path::Path::new(&a[1])).expect("vfs");
    let bytes = vfs.resolve_name("g3.efm").and_then(|s| vfs.read(s).ok()).expect("efm");
    let genome = GenomeFile::parse(&bytes).expect("genome");
    let strings = Strings::new(&genome.strings);
    let mut cur = Cursor::new(genome.body);
    let _v = cur.u16().unwrap();
    let count = cur.u32().unwrap() as usize;
    let mut stats = DecodeStats::default();
    let mut classes: std::collections::BTreeMap<String, usize> = Default::default();
    for _ in 0..count {
        let name = strings.entry(&mut cur).unwrap();
        let _v = cur.u16().unwrap();
        let n = cur.u32().unwrap() as usize;
        for _ in 0..n {
            let set = read_sub_class(&mut cur, &strings, 0, &mut stats).unwrap();
            *classes.entry(set.class.clone()).or_default() += 1;
            if set.class == "gCEffectCommandCreateDecal" {
                println!("{name}: {}", set.class);
                for p in &set.properties {
                    println!("    {} = {:?}", p.name, p.value);
                }
            }
        }
    }
    println!("{classes:?}");
}
