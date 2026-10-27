//! Why does a character look squashed? Compares an actor's bind pose with its idle pose, bone
//! by bone, and with its skeleton resource:
//!
//! ```text
//! cargo run --release -p gothic-animation --example shape_probe -- \
//!     [--data-dir DIR] [BODY [SKELETON]]
//! ```
//!
//! Defaults: `G3_Orc_Body_Warrior` against `G3_Orc_Skeleton`, then the hero
//! (`G3_Hero_Body_Player` / `G3_Hero_Skeleton`) as the reference. Reports:
//!
//! * standing height (highest minus lowest animated node, cm) in the bind pose and in the idle
//!   pose at t = 0 and mid-clip: an idle that is much lower than the bind pose crouches;
//! * the bones whose local offset the idle clip changes the most (clip translation against the
//!   bind length): clips authored for other proportions pull a body together;
//! * the bones whose bind offset differs most between the body and the skeleton resource the
//!   motions were made for;
//! * non-unit bind or posed scales.
//!
//! Prints numbers only; nothing from the data is written anywhere.
use anyhow::{Context, Result, anyhow};
use bevy::math::{Affine3A, Vec3};
use gothic_animation::ActorLibrary;
use gothic_animation::Stance;
use gothic_animation::locomotion::{LocoKey, MoveDir, MoveKind};
use gothic_animation::rig::{Rig, evaluate, is_helper};
use gothic_formats::{PakArchive, xact::Actor};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const DEFAULT_DATA_DIR: &str = r"D:\SteamLibrary\steamapps\common\Gothic 3\Data";

fn load_rig(pak: &mut PakArchive, stem: &str) -> Result<Rig> {
    let want = format!("{}.xact", stem.to_lowercase());
    let at = pak
        .entries
        .iter()
        .position(|e| {
            let p = e.path.to_lowercase();
            p == want || p.ends_with(&format!("/{want}")) || p.ends_with(&format!("\\{want}"))
        })
        .ok_or_else(|| anyhow!("{stem}.xact not in _compiledAnimation.pak"))?;
    let actor = Actor::parse(&pak.read_payload(at)?).with_context(|| format!("parsing {stem}"))?;
    Ok(Rig::from_actor(stem, &actor))
}

fn animated(rig: &Rig) -> impl Iterator<Item = usize> + '_ {
    (0..rig.names.len())
        .filter(|&i| !is_helper(&rig.names[i]) && !rig.names[i].starts_with("Slot_"))
}

fn height(rig: &Rig, model: &[Affine3A]) -> f32 {
    let ys: Vec<f32> = animated(rig).map(|i| model[i].translation.y).collect();
    let max = ys.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let min = ys.iter().cloned().fold(f32::INFINITY, f32::min);
    max - min
}

fn posed(
    rig: &Arc<Rig>,
    lib: &mut ActorLibrary,
    motion: &str,
    t: f32,
) -> Result<(Vec<Affine3A>, Vec<gothic_animation::pose::Trs>, f32)> {
    let m = lib.motion(rig, motion)?;
    let plan = rig.plan(&(0..rig.names.len() as u16).collect::<Vec<_>>());
    let mut pose = rig.rest.clone();
    let duration = m.clip.duration;
    m.binding
        .sample(&m.clip, &rig.rest, t * duration, &mut pose);
    let mut model = vec![Affine3A::IDENTITY; rig.names.len()];
    evaluate(&plan, &pose, &mut model);
    Ok((model, pose, duration))
}

fn report(dir: &Path, lib: &mut ActorLibrary, body: &str, skeleton: Option<&str>) -> Result<()> {
    let mut pak = PakArchive::open(dir.join("_compiledAnimation.pak"))?;
    let mut rig = load_rig(&mut pak, body)?;
    rig.family = lib.motion_family(&rig.family);
    let rig = Arc::new(rig);
    println!(
        "\n== {body} (motion family {:?}, {} nodes)",
        rig.family,
        rig.names.len()
    );

    let plan = rig.plan(&(0..rig.names.len() as u16).collect::<Vec<_>>());
    let mut bind = vec![Affine3A::IDENTITY; rig.names.len()];
    evaluate(&plan, &rig.rest, &mut bind);
    let bind_h = height(&rig, &bind);
    println!("bind height        {bind_h:8.1} cm");

    let idle = LocoKey {
        kind: MoveKind::Idle,
        dir: MoveDir::Fwd,
        stance: Stance::Unarmed,
        sneak: false,
        parade: false,
        body: Default::default(),
    };
    let Some(motion) = lib.locomotion_motion(&rig.family, idle) else {
        println!("no idle motion for family {:?}", rig.family);
        return Ok(());
    };
    println!("idle motion        {motion}");
    let (model0, pose0, duration) = posed(&rig, lib, &motion, 0.0)?;
    let (model_mid, _, _) = posed(&rig, lib, &motion, 0.5)?;
    let h0 = height(&rig, &model0);
    let hm = height(&rig, &model_mid);
    println!(
        "idle height        {h0:8.1} cm at t=0 ({:.0} %), {hm:.1} cm mid-clip ({:.0} %), clip {duration:.2} s",
        100.0 * h0 / bind_h.max(1e-3),
        100.0 * hm / bind_h.max(1e-3)
    );

    // Bones whose local offset the clip changes most (slot order = rest order).
    let mut moved: Vec<(f32, String, f32, f32)> = Vec::new();
    let mut scaled: Vec<String> = Vec::new();
    for (slot, &node) in rig.slot_node.iter().enumerate() {
        let name = &rig.names[node as usize];
        let rest = rig.rest[slot].t.length();
        let now = pose0[slot].t.length();
        if rest > 1.0 {
            moved.push(((now / rest - 1.0).abs(), name.clone(), rest, now));
        }
        let (rs, ps) = (rig.rest[slot].s, pose0[slot].s);
        if (rs - Vec3::ONE).abs().max_element() > 1e-3
            || (ps - Vec3::ONE).abs().max_element() > 1e-3
        {
            scaled.push(format!("{name}: bind {rs:.3?} posed {ps:.3?}"));
        }
    }
    moved.sort_by(|a, b| b.0.total_cmp(&a.0));
    println!("bones whose length the idle changes most (bind -> idle, cm):");
    for (_, name, rest, now) in moved.iter().take(12) {
        println!(
            "  {name:36} {rest:7.1} -> {now:7.1}  ({:+.0} %)",
            100.0 * (now / rest - 1.0)
        );
    }
    if scaled.is_empty() {
        println!("scales: all 1");
    } else {
        println!("non-unit scales:");
        for s in scaled.iter().take(12) {
            println!("  {s}");
        }
    }

    if let Some(skel) = skeleton {
        match load_rig(&mut pak, skel) {
            Ok(srig) => {
                let mut sbind = vec![Affine3A::IDENTITY; srig.names.len()];
                let splan = srig.plan(&(0..srig.names.len() as u16).collect::<Vec<_>>());
                evaluate(&splan, &srig.rest, &mut sbind);
                println!("{skel}: bind height {:.1} cm", height(&srig, &sbind));
                let mut diff: Vec<(f32, String, f32, f32)> = Vec::new();
                let mut missing = 0;
                for i in animated(&rig) {
                    let Some(j) = srig.node(&rig.names[i]) else {
                        missing += 1;
                        continue;
                    };
                    let a = rig.bind_local[i].t.length();
                    let b = srig.bind_local[j as usize].t.length();
                    if a > 1.0 || b > 1.0 {
                        diff.push(((a / b.max(1e-3) - 1.0).abs(), rig.names[i].clone(), a, b));
                    }
                }
                diff.sort_by(|a, b| b.0.total_cmp(&a.0));
                println!(
                    "bind offsets body vs skeleton (cm), {missing} body nodes not in the skeleton:"
                );
                for (_, name, a, b) in diff.iter().take(12) {
                    println!("  {name:36} {a:7.1} vs {b:7.1}");
                }
            }
            Err(e) => println!("{skel}: {e:#}"),
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut dir = PathBuf::from(DEFAULT_DATA_DIR);
    if let Some(i) = args.iter().position(|a| a == "--data-dir") {
        dir = PathBuf::from(
            args.get(i + 1)
                .ok_or_else(|| anyhow!("--data-dir needs a path"))?,
        );
        args.drain(i..=i + 1);
    }
    let mut lib = ActorLibrary::open(&dir)?;
    match args.as_slice() {
        [] => {
            report(
                &dir,
                &mut lib,
                "G3_Orc_Body_Warrior",
                Some("G3_Orc_Skeleton"),
            )?;
            report(
                &dir,
                &mut lib,
                "G3_Hero_Body_Player",
                Some("G3_Hero_Skeleton"),
            )?;
        }
        [body] => report(&dir, &mut lib, body, None)?,
        [body, skel, ..] => report(&dir, &mut lib, body, Some(skel))?,
    }
    Ok(())
}
