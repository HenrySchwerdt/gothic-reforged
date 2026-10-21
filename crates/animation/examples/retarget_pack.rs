//! Retargets a local `SANM` source animation set onto the Gothic 3 hero skeleton and writes a
//! clip pack: `retarget_pack SRC.sanm OUT.grcp [--timing actions.json]`.
//!
//! The source set and the pack stay on the user's machine (gitignored `.local/`); this tool only
//! contains the generic bone table of a humanoid source skeleton with Havok-style names.
use bevy::math::{Mat3, Quat, Vec3};
use gothic_animation::{
    clip_pack::{ClipPack, ClipTiming, PackClip},
    retarget::{Grip, Options, Pair, Retargeter, SrcAnims},
    rig::Rig,
};
use gothic_formats::{PakArchive, xact::Actor};

fn pairs() -> Vec<Pair> {
    let mut v = vec![
        Pair::new("Hero_Spine_Spine_1", "Pelvis"),
        Pair::new("Hero_Spine_Spine_2", "Spine"),
        Pair::new("Hero_Spine_Spine_3", "Spine1"),
        Pair::new("Hero_Spine_Spine_4", "Spine2"),
        Pair::new("Hero_Head_Neck_1", "Neck"),
        Pair::new("Hero_Head_Head_1", "Head"),
    ];
    for (g, e) in [("Left", "L"), ("Right", "R")] {
        v.push(Pair::new(
            &format!("Hero_{g}_Arm_Shoulder_1"),
            &format!("{e}_Clavicle"),
        ));
        v.push(Pair::new(
            &format!("Hero_{g}_Arm_Arm_1"),
            &format!("{e}_UpperArm"),
        ));
        v.push(Pair::new(
            &format!("Hero_{g}_Arm_Arm_2"),
            &format!("{e}_Forearm"),
        ));
        v.push(Pair::new(
            &format!("Hero_{g}_Hand_Hand_1"),
            &format!("{e}_Hand"),
        ));
        v.push(Pair::new(
            &format!("Hero_{g}_Leg_Leg_1"),
            &format!("{e}_Thigh"),
        ));
        v.push(Pair::new(
            &format!("Hero_{g}_Leg_Leg_2"),
            &format!("{e}_Calf"),
        ));
        v.push(Pair::new(
            &format!("Hero_{g}_Foot_Foot_1"),
            &format!("{e}_Foot"),
        ));
        v.push(Pair::new(
            &format!("Hero_{g}_Foot_Toe_1"),
            &format!("{e}_Toe0"),
        ));
    }
    v
}

fn aims() -> Vec<(String, String)> {
    let mut v: Vec<(&str, &str)> = vec![
        ("Hero_Spine_Spine_1", "Hero_Spine_Spine_2"),
        ("Hero_Spine_Spine_2", "Hero_Spine_Spine_3"),
        ("Hero_Spine_Spine_3", "Hero_Spine_Spine_4"),
        ("Hero_Spine_Spine_4", "Hero_Head_Neck_1"),
        ("Hero_Head_Neck_1", "Hero_Head_Head_1"),
    ];
    let mut out: Vec<(String, String)> = v.drain(..).map(|(a, b)| (a.into(), b.into())).collect();
    for g in ["Left", "Right"] {
        for (a, b) in [
            (
                format!("Hero_{g}_Arm_Shoulder_1"),
                format!("Hero_{g}_Arm_Arm_1"),
            ),
            (format!("Hero_{g}_Arm_Arm_1"), format!("Hero_{g}_Arm_Arm_2")),
            (
                format!("Hero_{g}_Arm_Arm_2"),
                format!("Hero_{g}_Hand_Hand_1"),
            ),
            (format!("Hero_{g}_Leg_Leg_1"), format!("Hero_{g}_Leg_Leg_2")),
            (
                format!("Hero_{g}_Leg_Leg_2"),
                format!("Hero_{g}_Foot_Foot_1"),
            ),
            (
                format!("Hero_{g}_Foot_Foot_1"),
                format!("Hero_{g}_Foot_Toe_1"),
            ),
        ] {
            out.push((a, b));
        }
    }
    out
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let src = SrcAnims::from_bytes(&std::fs::read(&args[1])?).map_err(anyhow::Error::msg)?;
    let dir = std::path::Path::new(r"D:\SteamLibrary\steamapps\common\Gothic 3\Data");
    let mut pak = PakArchive::open(dir.join("_compiledAnimation.pak"))?;
    let at = pak
        .entries
        .iter()
        .position(|e| e.path.to_lowercase() == "g3_hero_body_player.xact")
        .unwrap();
    let rig = Rig::from_actor("G3_Hero_Body_Player", &Actor::parse(&pak.read_payload(at)?)?);

    // Scale: thigh-joint-to-ankle length ratio of the two skeletons.
    let sk = &src.skeleton;
    let (swp, _) = sk.world(&sk.rest_t, &sk.rest_q);
    let leg_src = (swp[sk.index("L_Thigh").unwrap()] - swp[sk.index("L_Foot").unwrap()]).length();
    let gp = |n: &str| {
        rig.bind_world[rig.node(n).unwrap() as usize]
            .w_axis
            .truncate()
    };
    let leg_g = (gp("Hero_Left_Leg_Leg_1") - gp("Hero_Left_Foot_Foot_1")).length();
    let scale = leg_g / leg_src;
    eprintln!("scale {scale:.2} (leg {leg_src:.3} -> {leg_g:.1})");

    let opt = Options {
        basis: Mat3::IDENTITY,
        frame: Quat::from_rotation_y(std::f32::consts::PI),
        scale,
        pairs: pairs(),
        root: Pair::new("Hero_Spine_Spine_1", "Pelvis"),
        feet: Some([
            Pair::new("Hero_Left_Foot_Toe_1", "L_Toe0"),
            Pair::new("Hero_Right_Foot_Toe_1", "R_Toe0"),
        ]),
        aim: aims(),
        offsets: vec![],
        // The sword mesh points along +Z of its slot; the source weapon bone's blade runs
        // along its +Y.
        grips: vec![Grip {
            hand: "Hero_Right_Hand_Hand_1".into(),
            slot: "Slot_RightHand_Weapon".into(),
            slot_axis: Vec3::Z,
            src_bone: "R_Weapon".into(),
            src_axis: Vec3::Y,
        }],
    };
    let rt = Retargeter::new(&rig, &src.skeleton, &opt).map_err(anyhow::Error::msg)?;
    eprintln!(
        "mapped {} bones; unmapped animated target bones: {}; missing source: {:?}",
        rt.report.mapped,
        rt.report.unmapped_target.len(),
        rt.report.missing_source
    );

    // Optional timing metadata (JSON written by the extraction tool).
    let timing = args
        .iter()
        .position(|a| a == "--timing")
        .and_then(|i| args.get(i + 1))
        .map(|p| std::fs::read_to_string(p))
        .transpose()?
        .map(|t| gothic_animation::clip_pack::parse_timing(&t))
        .unwrap_or_default();

    if let Some(only) = args.iter().position(|a| a == "--errors").and_then(|i| args.get(i + 1)) {
        for c in src.clips.iter().filter(|c| c.name == *only) {
            for (bone, mean, max) in rt.error_report(c) {
                println!("{bone:34} mean {mean:6.1} max {max:6.1}");
            }
        }
        for c in src.clips.iter().filter(|c| c.name == *only) {
            let (_, stats) = rt.clip_with_stats(c);
            for (f, s) in stats.iter().enumerate().step_by(4) {
                println!("frame {f:3} src toe {:6.1} tgt toe {:6.1} dy {:6.1}", s[0], s[1], s[2]);
            }
        }
        return Ok(());
    }
    let mut pack = ClipPack::default();
    for c in &src.clips {
        let mut clip = rt.clip(c);
        clip.name = format!("ER_{}", c.name);
        let looping = ["Walk_", "Run_", "Sprint", "_Idle", "_Guard"]
            .iter()
            .any(|k| c.name.contains(k));
        clip.looping = looping;
        let tm = timing.get(&c.name).cloned();
        pack.clips.push(PackClip {
            clip,
            fps: c.fps,
            timing: tm,
        });
    }
    std::fs::write(&args[2], pack.to_bytes())?;
    eprintln!("wrote {} clips to {}", pack.clips.len(), args[2]);
    let _ = Vec3::ZERO;
    Ok(())
}
