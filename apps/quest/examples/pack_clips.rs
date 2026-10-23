//! Host tool (reads the user's Gothic 3 install read-only, writes under the gitignored `.local/`): collects the
//! original locomotion and hit-reaction clips of the hero and the orc into `clips_hero.grcp` / `clips_orc.grcp`
//! (`gothic_animation::clip_pack::ClipPack`, mirrored like the skeletons) next to the Ardea data pack.
//! Usage: pack_clips [OUT_DIR]   (default .local/quest/ardea-pack)
use gothic_animation::ActorLibrary;
use gothic_animation::clip_pack::{ClipPack, PackClip};
use gothic_animation::names::MotionQuery;

struct Want {
    left: &'static str,
    right: &'static str,
    action: &'static str,
    phase: &'static str,
    dir: &'static str,
}
const fn w(
    left: &'static str,
    right: &'static str,
    action: &'static str,
    phase: &'static str,
    dir: &'static str,
) -> Want {
    Want {
        left,
        right,
        action,
        phase,
        dir,
    }
}

fn main() -> anyhow::Result<()> {
    let out = std::path::PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| ".local/quest/ardea-pack".into()),
    );
    let lib_dir = std::path::Path::new(r"D:\SteamLibrary\steamapps\common\Gothic 3\Data");
    let mut lib = ActorLibrary::open(lib_dir)?;
    let mut hero: Vec<Want> = vec![w("None", "None", "Ambient", "Loop", "Fwd")];
    for phase in ["Walk", "Run"] {
        for dir in ["Fwd", "Back", "Left", "Right"] {
            hero.push(w("None", "None", "Move", phase, dir));
        }
    }
    let mut orc: Vec<Want> = vec![w("None", "2H", "Ambient", "Loop", "Fwd")];
    for phase in ["Walk", "Run"] {
        for dir in ["Fwd", "Back", "Left", "Right"] {
            orc.push(w("None", "2H", "Move", phase, dir));
        }
    }
    for action in ["QuickStumble", "StumbleL", "StumbleR", "PierceStumble"] {
        for phase in ["Hit", "Recover"] {
            orc.push(w("None", "2H", action, phase, "Fwd"));
        }
    }
    for action in ["Attack", "PowerAttack"] {
        for phase in ["Raise", "Hit", "Recover"] {
            orc.push(w("None", "2H", action, phase, "Fwd"));
        }
    }
    for (family, wants, file) in [
        ("Hero", hero, "clips_hero.grcp"),
        ("Orc", orc, "clips_orc.grcp"),
    ] {
        let mut pack = ClipPack::default();
        for want in &wants {
            let q = MotionQuery {
                pose: Some("Stand"),
                left: Some(want.left),
                right: Some(want.right),
                pn: Some("P0"),
                action: Some(want.action),
                phase: Some(want.phase),
                dir: Some(want.dir),
                ..Default::default()
            };
            let Some(name) = lib.catalog.find(family, &q).map(str::to_string) else {
                println!(
                    "{family}: no clip for {} {} {} {} {}",
                    want.left, want.right, want.action, want.phase, want.dir
                );
                continue;
            };
            let clip = lib.clip(&name)?;
            println!(
                "{family}: {name} ({:.2}s, looping {}, {} nodes)",
                clip.duration,
                clip.looping,
                clip.nodes.len()
            );
            pack.clips.push(PackClip {
                clip: (*clip).clone(),
                fps: 30.0,
                timing: None,
            });
        }
        let bytes = pack.to_bytes();
        std::fs::write(out.join(file), &bytes)?;
        println!(
            "wrote {} ({} clips, {} KB)",
            out.join(file).display(),
            pack.clips.len(),
            bytes.len() / 1024
        );
    }
    Ok(())
}
