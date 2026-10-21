//! Prints a rig: node, parent, effective parent, helper flag, bind-world position and the
//! motion-relative rest pose. `rig_dump ACTOR_NAME`
use gothic_animation::rig::Rig;
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
    let rig = Rig::from_actor(&name, &a);
    for i in 0..rig.names.len() {
        let w = rig.bind_world[i].to_scale_rotation_translation();
        let q = w.1;
        println!(
            "{i:3} {:40} par {:4} eff {:4} {} pos {:7.1} {:7.1} {:7.1} q {:6.3} {:6.3} {:6.3} {:6.3}",
            rig.names[i],
            rig.parent[i] as i32,
            rig.eff_parent[i] as i16,
            if rig.slot_of[i] == gothic_animation::rig::NONE {
                "H"
            } else {
                "A"
            },
            w.2.x,
            w.2.y,
            w.2.z,
            q.x,
            q.y,
            q.z,
            q.w
        );
    }
    Ok(())
}
