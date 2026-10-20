use gothic_formats::{PakArchive, xact::Actor, xmot::Motion};
use std::collections::BTreeSet;
fn main() -> anyhow::Result<()> {
    let dir = r"D:\SteamLibrary\steamapps\common\Gothic 3\Data";
    let mut pak = PakArchive::open(format!(r"{dir}\_compiledAnimation.pak"))?;
    let find = |pak: &PakArchive, s: &str| pak.entries.iter().position(|e| e.path.to_lowercase() == s.to_lowercase()).unwrap();
    let a = Actor::parse(&pak.read_payload(find(&pak, "G3_Hero_Body_Player.xact"))?)?;
    for (i, n) in a.nodes.iter().enumerate() {
        println!("{i} {} <- {} t={:?}", n.name, n.parent_name, n.position);
    }
    let args: Vec<String> = std::env::args().collect();
    let mut union = BTreeSet::new();
    for m in &args[1..] {
        let mo = Motion::parse(&pak.read_payload(find(&pak, &format!("{m}.xmot")))?)?;
        println!("motion {m} dur {} nodes {}", mo.duration, mo.nodes.len());
        for n in &mo.nodes {
            union.insert(n.name.clone());
            if n.positions.keys.len() > 0 {
                let k = &n.positions.keys;
                println!("  poskeys {} {} {:?} -> {:?}", n.name, k.len(), k[0], k[k.len() - 1]);
            }
        }
    }
    println!("{:?}", union);
    Ok(())
}
