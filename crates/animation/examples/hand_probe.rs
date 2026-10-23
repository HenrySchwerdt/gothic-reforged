//! Hand mesh extent vs slot position in the authored bind pose.
use gothic_animation::rig::Rig;
use gothic_formats::{PakArchive, xact::Actor};
fn main() -> anyhow::Result<()> {
    let dir = std::path::Path::new(r"D:\SteamLibrary\steamapps\common\Gothic 3\Data");
    let mut pak = PakArchive::open(dir.join("_compiledAnimation.pak"))?;
    let at = pak
        .entries
        .iter()
        .position(|e| e.path.to_lowercase() == "g3_hero_body_player.xact")
        .unwrap();
    let a = Actor::parse(&pak.read_payload(at)?)?;
    let rig = Rig::from_actor_source("b", &a);
    let mut sum = [0f32; 3];
    let mut n = 0;
    let mut lo = [1e9f32; 3];
    let mut hi = [-1e9f32; 3];
    for m in &a.meshes {
        for s in &m.submeshes {
            for (i, p) in s.positions.iter().enumerate() {
                let (mut best, mut bw) = (0u16, -1.0);
                for k in 0..4 {
                    if s.weights[i][k] > bw {
                        bw = s.weights[i][k];
                        best = s.joints[i][k];
                    }
                }
                let nm = &rig.names[best as usize];
                if nm.starts_with("Hero_Right_Hand_") {
                    n += 1;
                    for c in 0..3 {
                        sum[c] += p[c];
                        lo[c] = lo[c].min(p[c]);
                        hi[c] = hi[c].max(p[c]);
                    }
                }
            }
        }
    }
    println!(
        "right hand verts {n} centroid {:?} lo {:?} hi {:?}",
        sum.map(|x| x / n as f32),
        lo,
        hi
    );
    for s in [
        "Hero_Right_Hand_Hand_1",
        "Slot_RightHand_Weapon",
        "Slot_LeftHand_Weapon",
        "Hero_Left_Hand_Hand_1",
    ] {
        let i = rig.node(s).unwrap() as usize;
        println!(
            "{s}: {:?} local {:?}",
            rig.bind_world[i].w_axis, rig.bind_local[i]
        );
    }
    Ok(())
}
