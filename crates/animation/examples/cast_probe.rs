//! Which arm does a cast clip move? `cast_probe NEEDLE...` lists the Hero clips whose name
//! contains every needle and, per clip, the largest rotation travel of the left and right arm
//! chains (angle between the first pose and the most different pose of the clip, summed over the
//! upper arm, forearm and hand nodes).
use gothic_animation::ActorLibrary;
use gothic_formats::PakArchive;

fn main() -> anyhow::Result<()> {
    let needles: Vec<String> = std::env::args().skip(1).map(|s| s.to_lowercase()).collect();
    let dir = std::path::Path::new(r"D:\SteamLibrary\steamapps\common\Gothic 3\Data");
    let pak = PakArchive::open(dir.join("_compiledAnimation.pak"))?;
    let mut lib = ActorLibrary::open(dir)?;
    let mut names: Vec<String> = pak
        .entries
        .iter()
        .map(|e| e.path.clone())
        .filter(|p| p.to_lowercase().ends_with(".xmot"))
        .map(|p| p.trim_end_matches(".xmot").to_string())
        .filter(|p| {
            let l = p.to_lowercase();
            l.starts_with("hero_") && needles.iter().all(|n| l.contains(n))
        })
        .collect();
    names.sort();
    names.dedup();
    for n in names {
        let Ok(clip) = lib.clip(&n) else { continue };
        let mut left = 0.0f32;
        let mut right = 0.0f32;
        for (key, &i) in &clip.by_name {
            let k = key.to_lowercase();
            let side_left = k.contains("left") || k.contains("_l_") || k.ends_with("_l");
            let side_right = k.contains("right") || k.contains("_r_") || k.ends_with("_r");
            if !(k.contains("arm") || k.contains("hand")) || !(side_left || side_right) {
                continue;
            }
            let node = &clip.nodes[i as usize];
            let r0 = node.sample(0.0).r;
            let mut worst = 0.0f32;
            let steps = 20;
            for s in 1..=steps {
                let t = clip.duration * s as f32 / steps as f32;
                worst = worst.max(r0.angle_between(node.sample(t).r));
            }
            if side_left {
                left += worst;
            } else {
                right += worst;
            }
        }
        println!(
            "{n}: dur {:.2}s left arm {:.0} deg, right arm {:.0} deg",
            clip.duration,
            left.to_degrees(),
            right.to_degrees()
        );
    }
    Ok(())
}
