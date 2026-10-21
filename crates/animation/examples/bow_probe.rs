//! Bow actor: rest pose vs Relax/Cock clip samples per bone: bow_probe
use gothic_animation::{ActorLibrary, rig::Rig};
use gothic_formats::{PakArchive, xact::Actor};
use std::sync::Arc;
fn main() -> anyhow::Result<()> {
    let dir = std::path::Path::new(r"D:\SteamLibrary\steamapps\common\Gothic 3\Data");
    let mut pak = PakArchive::open(dir.join("_compiledAnimation.pak"))?;
    let base = "It_Weapon_Ranged_Bow_Hunting_01";
    let at = pak.entries.iter().position(|e| e.path.to_lowercase() == format!("{}.xact", base.to_lowercase())).unwrap();
    let rig = Arc::new(Rig::from_actor(base, &Actor::parse(&pak.read_payload(at)?)?));
    let mut lib = ActorLibrary::open(dir)?;
    for (slot, &n) in rig.slot_node.iter().enumerate() {
        let r = rig.rest[slot];
        println!("rest {} t {:?} r {:?} eff {}", rig.names[n as usize], r.t, r.r, rig.eff_parent[n as usize] as i16);
    }
    for suffix in ["Relax", "Cock"] {
        let name = format!("{base}_{suffix}");
        let m = lib.motion(&rig, &name)?;
        println!("{name} dur {} bound {}", m.clip.duration, m.binding.bound);
        for t in [0.0, m.clip.duration] {
            for (slot, &n) in rig.slot_node.iter().enumerate() {
                let c = m.binding.node_of_slot[slot];
                if c == u16::MAX { println!("  t={t} {} unbound", rig.names[n as usize]); continue; }
                let s = m.clip.nodes[c as usize].sample(t);
                println!("  t={t} {} t {:?} r {:?}", rig.names[n as usize], s.t, s.r);
            }
        }
    }
    Ok(())
}
