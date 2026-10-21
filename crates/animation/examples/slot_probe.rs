//! Checks that helper/slot nodes follow their real parent: slot_probe [MOTION_SUBSTR]
use bevy::math::{Affine3A, Mat4};
use gothic_animation::ActorLibrary;
use gothic_animation::rig::{NONE, Rig};
use gothic_formats::{PakArchive, xact::Actor};
use std::sync::Arc;
fn main() -> anyhow::Result<()> {
    let filter = std::env::args().nth(1).unwrap_or("Aim_Aim".into());
    let dir = std::path::Path::new(r"D:\SteamLibrary\steamapps\common\Gothic 3\Data");
    let mut pak = PakArchive::open(dir.join("_compiledAnimation.pak"))?;
    let at = pak
        .entries
        .iter()
        .position(|e| e.path.to_lowercase() == "g3_hero_body_player.xact")
        .unwrap();
    let rig = Arc::new(Rig::from_actor(
        "G3_Hero_Body_Player",
        &Actor::parse(&pak.read_payload(at)?)?,
    ));
    let mut lib = ActorLibrary::open(dir)?;
    let names: Vec<String> = lib
        .catalog
        .motions("hero")
        .map(|(n, _)| n.to_string())
        .filter(|n| n.contains(&filter))
        .take(3)
        .collect();
    let actor = Actor::parse(&pak.read_payload(at)?)?;
    let plan = rig.plan(&(0..rig.names.len() as u16).collect::<Vec<_>>());
    for name in names {
        let m = lib.motion(&rig, &name)?;
        for tt in [0.0f32, m.clip.duration * 0.5, m.clip.duration] {
            println!(" t={tt} dur={}", m.clip.duration);
            let mut pose = rig.rest.clone();
            m.binding.sample(&m.clip, &rig.rest, tt, &mut pose);
            let mut model = vec![Affine3A::IDENTITY; rig.names.len()];
            gothic_animation::rig::evaluate(&plan, &pose, &mut model);
            println!("{name} bound {}/{}", m.binding.bound, rig.slot_node.len());
            for s in ["Slot_RightHand_Weapon", "Slot_LeftHand_Weapon", "Slot_Head"] {
                let Some(i) = rig.node(s) else { continue };
                let p = rig.parent[i as usize];
                let want =
                    Mat4::from(model[p as usize]) * Mat4::from(rig.bind_local[i as usize].affine());
                let got = model[i as usize].translation;
                let pp = model[p as usize].translation;
                println!(
                    " {s}: parent {} pos {:?} parentpos {:?} want {:?}",
                    rig.names[p as usize], got, pp, want.w_axis
                );
            }
            // Skinned right-hand mesh centroid (vertices mirrored like the game's meshes).
            let mut sum = bevy::math::Vec3::ZERO;
            let mut cnt = 0;
            for mesh in &actor.meshes {
                for sub in &mesh.submeshes {
                    for (i, p) in sub.positions.iter().enumerate() {
                        let mut best = 0usize;
                        let mut bw = -1.0;
                        for k in 0..4 {
                            if sub.weights[i][k] > bw {
                                bw = sub.weights[i][k];
                                best = sub.joints[i][k] as usize;
                            }
                        }
                        if !rig.names[best].starts_with("Hero_Right_Hand_") {
                            continue;
                        }
                        let v = bevy::math::Vec3::new(-p[0], p[1], p[2]);
                        let mut acc = bevy::math::Vec3::ZERO;
                        for k in 0..4 {
                            let j = sub.joints[i][k] as usize;
                            let w = sub.weights[i][k];
                            let sk = Mat4::from(model[j]) * rig.bind_world[j].inverse();
                            acc += w * sk.transform_point3(v);
                        }
                        sum += acc;
                        cnt += 1;
                    }
                }
            }
            println!(
                "  right hand mesh centroid {:?} ({cnt} verts)",
                sum / cnt as f32
            );
        if let Some(i) = rig.node("Slot_LeftHand_Weapon") {
            let a = model[i as usize].matrix3;
            println!("  left slot axes X {:?} Y {:?} Z {:?}", a.x_axis, a.y_axis, a.z_axis);
        }
        }
        for (slot, &n) in rig.slot_node.iter().enumerate() {
            if m.binding.node_of_slot[slot] == NONE {
                println!("  unbound {}", rig.names[n as usize]);
            }
        }
    }
    Ok(())
}
