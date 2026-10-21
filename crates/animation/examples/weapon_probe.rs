//! Weapon attachment geometry: the slot node's axes in the rest pose against the forearm
//! direction, and the mesh's long axis and head side: weapon_probe BODY_ACTOR MESH
use bevy::math::{Affine3A, Vec3};
use gothic_animation::ActorLibrary;
use gothic_animation::rig::Rig;
use gothic_formats::{PakArchive, xact::Actor};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let body = &args[1];
    let mesh = &args[2];
    let dir = std::path::Path::new(r"D:\SteamLibrary\steamapps\common\Gothic 3\Data");
    let mut pak = PakArchive::open(dir.join("_compiledAnimation.pak"))?;
    let at = pak
        .entries
        .iter()
        .position(|e| e.path.to_lowercase() == format!("{}.xact", body.to_lowercase()))
        .expect("body actor");
    let actor = Actor::parse(&pak.read_payload(at)?)?;
    let rig = Rig::from_actor(body, &actor);
    let plan = rig.plan(&(0..rig.names.len() as u16).collect::<Vec<_>>());
    let mut model = vec![Affine3A::IDENTITY; rig.names.len()];
    gothic_animation::rig::evaluate(&plan, &rig.rest, &mut model);
    for s in ["Slot_RightHand_Weapon", "Slot_LeftHand_Weapon"] {
        let Some(i) = rig.node(s) else {
            println!("{s}: missing");
            continue;
        };
        let m = model[i as usize];
        let p = rig.parent[i as usize] as usize;
        let hand = model[p].translation;
        // The bone chain up: hand -> forearm -> upper arm.
        let mut chain = vec![];
        let mut c = p;
        for _ in 0..3 {
            chain.push((rig.names[c].clone(), Vec3::from(model[c].translation)));
            let up = rig.parent[c];
            if up == gothic_animation::rig::NONE {
                break;
            }
            c = up as usize;
        }
        let fore = (Vec3::from(hand) - chain.get(1).map_or(Vec3::ZERO, |x| x.1)).normalize_or_zero();
        println!("{s}: pos {:?}", Vec3::from(m.translation));
        println!("  X axis {:?}", m.matrix3.x_axis.normalize());
        println!("  Y axis {:?}", m.matrix3.y_axis.normalize());
        println!("  Z axis {:?}", m.matrix3.z_axis.normalize());
        println!("  forearm direction (hand - parent chain): {fore:?}  chain {chain:?}");
    }
    let mut lib = ActorLibrary::open(dir)?;
    if let Some(clip_name) = std::env::var_os("CLIP") {
        let clip = lib.clip(&clip_name.to_string_lossy())?;
        let slots: Vec<&String> = clip
            .by_name
            .keys()
            .filter(|k| k.to_lowercase().starts_with("slot_"))
            .collect();
        println!("clip {}: {} node tracks, slot tracks: {slots:?}", clip.name, clip.nodes.len());
    }
    // The mesh: bounds and the head side of the long axis, as the game mirrors it (x -> -x).
    let mut mp = PakArchive::open(dir.join("_compiledMesh.pak"))?;
    let mi = mp
        .entries
        .iter()
        .position(|e| {
            e.path
                .to_lowercase()
                .ends_with(&format!("{}.xcmsh", mesh.to_lowercase()))
        })
        .expect("mesh");
    let x = gothic_formats::xcmsh::Xcmsh::parse(&mp.read_payload(mi)?)?;
    let mut pts: Vec<Vec3> = Vec::new();
    for sub in &x.submeshes {
        for p in &sub.positions {
            pts.push(Vec3::new(-p[0], p[1], p[2]));
        }
    }
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for p in &pts {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    println!("mesh {mesh}: min {lo:?} max {hi:?} (mirrored)");
    let ext = hi - lo;
    let long = if ext.x >= ext.y && ext.x >= ext.z { 0 } else if ext.y >= ext.z { 1 } else { 2 };
    println!("long axis {long}, extent {ext:?}");
    // The thick end: the half of the vertices with the larger cross-section spread.
    let mid = (lo[long] + hi[long]) * 0.5;
    let spread = |sel: &dyn Fn(f32) -> bool| {
        let v: Vec<&Vec3> = pts.iter().filter(|p| sel(p[long])).collect();
        let c = v.iter().fold(Vec3::ZERO, |a, p| a + **p) / v.len().max(1) as f32;
        v.iter().map(|p| (**p - c).length()).sum::<f32>() / v.len().max(1) as f32
    };
    // Cross-section by slice along the long axis.
    let bins = 12usize;
    for b in 0..bins {
        let a = lo[long] + ext[long] * b as f32 / bins as f32;
        let z = lo[long] + ext[long] * (b + 1) as f32 / bins as f32;
        let (mut mx, mut cnt) = (0.0f32, 0);
        let (mut sx, mut sy) = (0.0f32, 0.0f32);
        for p in pts.iter().filter(|p| p[long] >= a && p[long] < z) {
            let o: Vec<f32> = (0..3).filter(|i| *i != long).map(|i| p[i]).collect();
            mx = mx.max(o[0].abs().max(o[1].abs()));
            sx += o[0];
            sy += o[1];
            cnt += 1;
        }
        println!(
            "  slice {a:7.1}..{z:7.1}: n {cnt:5} max cross {mx:5.1} mean cross ({:.1}, {:.1})",
            sx / cnt.max(1) as f32,
            sy / cnt.max(1) as f32
        );
    }
    println!(
        "cross-section spread: low half {:.1}, high half {:.1} (the head is the larger one)",
        spread(&|a| a < mid),
        spread(&|a| a >= mid)
    );
    Ok(())
}
