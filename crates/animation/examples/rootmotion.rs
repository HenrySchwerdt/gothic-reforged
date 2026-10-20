//! Prints root motion over the full length of a few clips (debug aid).
use gothic_animation::{
    clip::{Clip, ClipBinding, RootMotionSource},
    names::MotionName,
    rig::Rig,
};
use gothic_formats::{PakArchive, xact::Actor, xmot::Motion};

fn main() -> anyhow::Result<()> {
    let dir = r"D:\SteamLibrary\steamapps\common\Gothic 3\Data";
    let mut pak = PakArchive::open(format!(r"{dir}\_compiledAnimation.pak"))?;
    let at = pak.entries.iter().position(|e| e.path == "G3_Hero_Body_Player.xact").unwrap();
    let rig = Rig::from_actor("G3_Hero_Body_Player", &Actor::parse(&pak.read_payload(at)?)?);
    println!("family {} slots {}", rig.family, rig.slot_node.len());
    for name in std::env::args().skip(1) {
        let at = pak.entries.iter().position(|e| e.path == format!("{name}.xmot")).expect("clip");
        let motion = Motion::parse(&pak.read_payload(at)?)?;
        let mut clip = Clip::from_motion(&name, &motion);
        clip.looping = MotionName::parse(&name).is_some_and(|m| m.is_cyclic());
        let binding = ClipBinding::new(&rig, &clip);
        let d = RootMotionSource::delta(&binding, &clip, 0.0, clip.duration);
        println!("{name}: dur {:.2} bound {}/{} root_slot {} -> translation {:?} yaw {:.1} deg nominal {}", clip.duration, binding.bound, rig.slot_node.len(), binding.root_slot, d.translation, d.yaw.to_degrees(), clip.nominal_speed);
    }
    Ok(())
}
