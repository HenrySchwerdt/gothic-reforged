//! Which rotation of the bow in the left-hand slot makes the drawn pose consistent: the right
//! hand on the middle of the string, the arrow line pointing forward, the limbs vertical.
use bevy::math::{Affine3A, Mat3, Vec3};
use gothic_animation::ActorLibrary;
use gothic_animation::rig::Rig;
use gothic_formats::{PakArchive, xact::Actor};
use std::sync::Arc;

fn load(pak: &mut PakArchive, name: &str) -> anyhow::Result<Arc<Rig>> {
    let at = pak
        .entries
        .iter()
        .position(|e| e.path.to_lowercase() == format!("{}.xact", name.to_lowercase()))
        .expect("actor");
    Ok(Arc::new(Rig::from_actor(
        name,
        &Actor::parse(&pak.read_payload(at)?)?,
    )))
}

fn eval(rig: &Rig, m: &gothic_animation::animator::Motion, t: f32) -> Vec<Affine3A> {
    let plan = rig.plan(&(0..rig.names.len() as u16).collect::<Vec<_>>());
    let mut pose = rig.rest.clone();
    m.binding.sample(&m.clip, &rig.rest, t, &mut pose);
    let mut model = vec![Affine3A::IDENTITY; rig.names.len()];
    gothic_animation::rig::evaluate(&plan, &pose, &mut model);
    model
}

fn main() -> anyhow::Result<()> {
    let dir = std::path::Path::new(r"D:\SteamLibrary\steamapps\common\Gothic 3\Data");
    let mut pak = PakArchive::open(dir.join("_compiledAnimation.pak"))?;
    let hero = load(&mut pak, "G3_Hero_Body_Player")?;
    let bow = load(&mut pak, "It_Weapon_Ranged_Bow_Hunting_01")?;
    let mut lib = ActorLibrary::open(dir)?;
    let hm = lib.motion(
        &hero,
        "Hero_Stand_Bow_Arrow_P0_Aim_PowerRaise_O_Fwd_00_%_00_P0_0",
    )?;
    let h = eval(&hero, &hm, hm.clip.duration);
    let slot = h[hero.node("Slot_LeftHand_Weapon").unwrap() as usize];
    let right = Vec3::from(h[hero.node("Slot_RightHand_Weapon").unwrap() as usize].translation);
    let bm = lib.motion(&bow, "It_Weapon_Ranged_Bow_Hunting_01_Cock")?;
    let b = eval(&bow, &bm, bm.clip.duration);
    let a = Vec3::from(b[bow.node("Bone03").unwrap() as usize].translation);
    let c = Vec3::from(b[bow.node("Bone06").unwrap() as usize].translation);
    println!(
        "left slot at {:?}, right hand {:?}",
        slot.translation, right
    );
    println!("bow tips (bow space) {a:?} {c:?}");
    // All signed permutation matrices.
    let perms = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    for p in perms {
        for sx in [-1.0f32, 1.0] {
            for sy in [-1.0f32, 1.0] {
                for sz in [-1.0f32, 1.0] {
                    let signs = [sx, sy, sz];
                    let mut cols = [Vec3::ZERO; 3];
                    for (i, &pi) in p.iter().enumerate() {
                        let mut v = [0.0f32; 3];
                        v[pi] = signs[i];
                        cols[i] = Vec3::from(v);
                    }
                    let r = Mat3::from_cols(cols[0], cols[1], cols[2]);
                    let det = r.determinant();
                    let to_world = |v: Vec3| slot.transform_point3(r * v);
                    let (wa, wc) = (to_world(a), to_world(c));
                    let ab = wc - wa;
                    let t = (right - wa).dot(ab) / ab.length_squared();
                    let foot = wa + ab * t.clamp(0.0, 1.0);
                    let depth = foot.distance(right);
                    // arrow line: hand -> chord point; forward is +z of the character frame
                    let dir = (foot - right).normalize_or_zero();
                    let vertical = (ab.normalize().y).abs();
                    println!(
                        "det {det:+.0} perm {p:?} signs {signs:?}: t {t:.2} depth {depth:.0} arrow dir (x {:.2}, y {:.2}, z {:.2}) chord vertical {vertical:.2}",
                        dir.x, dir.y, dir.z
                    );
                }
            }
        }
    }
    Ok(())
}
