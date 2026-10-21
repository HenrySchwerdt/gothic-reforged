//! Prints model-space positions of key bones for a motion at time t: pose_probe MOTION [T]
//! (default: the Hero idle). Used to establish the frame conventions for retargeting.
use bevy::math::Affine3A;
use gothic_animation::{ActorLibrary, names::MotionQuery, rig::Rig};
use gothic_formats::{PakArchive, xact::Actor};
use std::sync::Arc;
fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let dir = std::path::Path::new(r"D:\SteamLibrary\steamapps\common\Gothic 3\Data");
    let mut pak = PakArchive::open(dir.join("_compiledAnimation.pak"))?;
    let at = pak
        .entries
        .iter()
        .position(|e| e.path.to_lowercase() == "g3_hero_skeleton.xact")
        .unwrap();
    let rig = Arc::new(Rig::from_actor(
        "G3_Hero_Skeleton",
        &Actor::parse(&pak.read_payload(at)?)?,
    ));
    let mut lib = ActorLibrary::open(dir)?;
    let name = match args.get(1) {
        Some(n) => n.clone(),
        None => lib
            .catalog
            .find(
                "Hero",
                &MotionQuery {
                    pose: Some("Stand"),
                    left: Some("None"),
                    right: Some("None"),
                    pn: Some("P0"),
                    action: Some("Ambient"),
                    phase: Some("Loop"),
                    ..Default::default()
                },
            )
            .unwrap()
            .to_string(),
    };
    let t: f32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.0);
    println!("motion {name} t={t}");
    let motion = lib.motion(&rig, &name)?;
    let plan = rig.plan(&(0..rig.names.len() as u16).collect::<Vec<_>>());
    let mut pose = rig.rest.clone();
    motion.binding.sample(&motion.clip, &rig.rest, t, &mut pose);
    let mut model = vec![Affine3A::IDENTITY; rig.names.len()];
    gothic_animation::rig::evaluate(&plan, &pose, &mut model);
    for n in [
        "Hero_Spine_Spine_1",
        "Hero_Spine_Spine_5",
        "Hero_Head_Head_1",
        "Hero_Left_Arm_Arm_1",
        "Hero_Right_Arm_Arm_1",
        "Hero_Left_Hand_Hand_1",
        "Hero_Left_Leg_Leg_1",
        "Hero_Right_Leg_Leg_1",
        "Hero_Left_Foot_Foot_1",
        "Hero_Right_Foot_Foot_1",
        "Hero_Left_Foot_Toe_1",
        "Hero_Right_Foot_Toe_1",
    ] {
        let i = rig.node(n).unwrap() as usize;
        let p = model[i].translation;
        let q = bevy::math::Quat::from_mat3a(&model[i].matrix3);
        println!(
            "{n:28} {:8.1} {:8.1} {:8.1}   q {:6.3} {:6.3} {:6.3} {:6.3}",
            p.x, p.y, p.z, q.x, q.y, q.z, q.w
        );
    }
    Ok(())
}
