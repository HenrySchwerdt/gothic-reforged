//! Where do the hands of the hero go during an aim clip: aim_probe CLIP [CLIP...]
//! Prints the left and right weapon slot in the character frame at a few times.
use gothic_animation::ActorLibrary;
use gothic_animation::rig::Rig;
use gothic_formats::{PakArchive, xact::Actor};
use std::sync::Arc;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let dir = std::path::Path::new(r"D:\SteamLibrary\steamapps\common\Gothic 3\Data");
    let mut pak = PakArchive::open(dir.join("_compiledAnimation.pak"))?;
    let at = pak
        .entries
        .iter()
        .position(|e| e.path.to_lowercase() == "g3_hero_body_player.xact")
        .expect("body");
    let rig = Arc::new(Rig::from_actor(
        "G3_Hero_Body_Player",
        &Actor::parse(&pak.read_payload(at)?)?,
    ));
    let mut lib = ActorLibrary::open(dir)?;
    let plan = rig.plan(&(0..rig.names.len() as u16).collect::<Vec<_>>());
    let left = rig.node("Slot_LeftHand_Weapon").expect("left") as usize;
    let right = rig.node("Slot_RightHand_Weapon").expect("right") as usize;
    for name in &args[1..] {
        let m = lib.motion(&rig, name)?;
        println!("{name}: dur {:.2}", m.clip.duration);
        for i in 0..=4 {
            let t = m.clip.duration * i as f32 / 4.0;
            let mut pose = rig.rest.clone();
            m.binding.sample(&m.clip, &rig.rest, t, &mut pose);
            let mut model = vec![bevy::math::Affine3A::IDENTITY; rig.names.len()];
            gothic_animation::rig::evaluate(&plan, &pose, &mut model);
            let l = model[left].translation;
            let r = model[right].translation;
            println!(
                "  t={t:.2} left (x {:.0}, y {:.0}, z {:.0})  right (x {:.0}, y {:.0}, z {:.0})",
                l.x, l.y, l.z, r.x, r.y, r.z
            );
        }
    }
    Ok(())
}
