//! Dumps node and mesh statistics of one actor: actor_dump NAME
use gothic_formats::{PakArchive, xact::Actor};
fn main() -> anyhow::Result<()> {
    let name = std::env::args().nth(1).unwrap();
    let mut pak =
        PakArchive::open(r"D:\SteamLibrary\steamapps\common\Gothic 3\Data\_compiledAnimation.pak")?;
    let at = pak
        .entries
        .iter()
        .position(|e| e.path.to_lowercase() == format!("{}.xact", name.to_lowercase()))
        .unwrap();
    let a = Actor::parse(&pak.read_payload(at)?)?;
    println!("nodes {} meshes {}", a.nodes.len(), a.meshes.len());
    for (i, n) in a.nodes.iter().enumerate().take(40) {
        println!("  node {i} {} parent {:?}", n.name, n.parent);
    }
    for m in &a.meshes {
        for s in &m.submeshes {
            let nz = s
                .weights
                .iter()
                .filter(|w| w.iter().any(|x| *x != 0.0))
                .count();
            println!(
                "  sub verts {} weighted {} joints {}",
                s.positions.len(),
                nz,
                s.joints.len()
            );
        }
    }
    Ok(())
}
