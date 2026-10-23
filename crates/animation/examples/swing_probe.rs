//! Which side of the weapon leads the swing: the slot node's velocity during the fastest part
//! of a strike clip, expressed in the slot frame: swing_probe BODY_ACTOR FAMILY CLIP_SUBSTR
use bevy::math::{Affine3A, Vec3};
use gothic_animation::ActorLibrary;
use gothic_animation::rig::Rig;
use gothic_formats::{PakArchive, xact::Actor};
use std::sync::Arc;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let (body, family, filter) = (&args[1], &args[2], &args[3]);
    let dir = std::path::Path::new(r"D:\SteamLibrary\steamapps\common\Gothic 3\Data");
    let mut pak = PakArchive::open(dir.join("_compiledAnimation.pak"))?;
    let at = pak
        .entries
        .iter()
        .position(|e| e.path.to_lowercase() == format!("{}.xact", body.to_lowercase()))
        .expect("body");
    let actor = Actor::parse(&pak.read_payload(at)?)?;
    let rig = Arc::new(Rig::from_actor(body, &actor));
    let mut lib = ActorLibrary::open(dir)?;
    let names: Vec<String> = lib
        .catalog
        .motions(family)
        .map(|(n, _)| n.to_string())
        .filter(|n| n.contains(filter))
        .take(6)
        .collect();
    let plan = rig.plan(&(0..rig.names.len() as u16).collect::<Vec<_>>());
    let slot = rig.node("Slot_RightHand_Weapon").expect("slot");
    for name in names {
        let m = lib.motion(&rig, &name)?;
        let n = 60;
        let mut frames: Vec<Affine3A> = Vec::new();
        for i in 0..=n {
            let t = m.clip.duration * i as f32 / n as f32;
            let mut pose = rig.rest.clone();
            m.binding.sample(&m.clip, &rig.rest, t, &mut pose);
            let mut model = vec![Affine3A::IDENTITY; rig.names.len()];
            gothic_animation::rig::evaluate(&plan, &pose, &mut model);
            frames.push(model[slot as usize]);
        }
        // Velocity in the slot frame of the fastest quarter of the frames.
        let mut v: Vec<(f32, Vec3)> = (0..n)
            .map(|i| {
                let d = Vec3::from(frames[i + 1].translation) - Vec3::from(frames[i].translation);
                let inv = frames[i].matrix3.inverse();
                (d.length(), Vec3::from(inv * bevy::math::Vec3A::from(d)))
            })
            .collect();
        v.sort_by(|a, b| b.0.total_cmp(&a.0));
        let top = &v[..n / 4];
        let sum = top.iter().fold(Vec3::ZERO, |a, x| a + x.1);
        let dir = sum.normalize_or_zero();
        // The head of the orc axe (mesh space, mirrored): centre (-24, 0, 50), cutting edge
        // out at x = -42 against the handle line x = 0. Does the edge lead the head's motion?
        let head = Vec3::new(-24.0, 0.0, 50.0);
        let mut lead = 0.0f32;
        let mut count = 0;
        let mut speeds: Vec<(f32, f32)> = Vec::new();
        for i in 0..n {
            let a = frames[i].transform_point3(head);
            let b = frames[i + 1].transform_point3(head);
            let d = b - a;
            let edge = frames[i].transform_vector3(Vec3::new(-1.0, 0.0, 0.0));
            speeds.push((d.length(), d.normalize_or_zero().dot(edge)));
        }
        speeds.sort_by(|a, b| b.0.total_cmp(&a.0));
        for (_, c) in speeds.iter().take(n / 4) {
            lead += c;
            count += 1;
        }
        println!(
            "  edge-leads cosine over the fastest quarter: {:.2} (positive: the cutting edge leads the head)",
            lead / count.max(1) as f32
        );
        println!(
            "{name}: dur {:.2}s, fastest-quarter swing direction in the slot frame (X, Y, Z) = ({:.2}, {:.2}, {:.2})",
            m.clip.duration, dir.x, dir.y, dir.z
        );
    }
    Ok(())
}
