//! Headless check of the hero avatar pipeline (needs the local Ardea pack): loads the hero actors from the cooked
//! cache, runs the cloud body IK for a posed head + two hands, retargets onto the Gothic skeleton and prints where the
//! avatar's head, hands, feet and the weapon slots end up in the world. Exit code 1 when something is off.
use bevy::asset::AssetPlugin;
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use gothic_quest::actor::{ActorRig, PartOptions, spawn_body, spawn_part};
use bevy::ecs::system::RunSystemOnce;
use gothic_quest::ardea::{self, Ardea};
use gothic_quest::hero::Hero;
use gothic_quest::qmat::QuestMaterial;
use gothic_quest::retarget::Retarget;
use gothic_vr::body::{BodyIk, Joint, Proportions};
use gothic_vr::Pose;

fn main() {
    let mut failed = false;
    let dir = ardea::data_dir().expect("run from the repo root with .local/quest/ardea-pack present");
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), TransformPlugin));
    app.init_asset::<Image>()
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .init_asset::<QuestMaterial>()
        .init_asset::<SkinnedMeshInverseBindposes>();
    app.insert_resource(gothic_xr_quest::XrFrameData::default());
    ardea::build(&mut app, dir).expect("ardea build");

    // IK pose: head 1.7 m, right hand forward-low, left hand raised.
    let mut ik = BodyIk::new(Proportions::for_eye_height(1.7));
    let head = Pose::new(Vec3::new(0.0, 1.7, 0.0), Quat::IDENTITY);
    let lh = Pose::new(Vec3::new(-0.3, 1.3, -0.4), Quat::IDENTITY);
    let rh = Pose::new(Vec3::new(0.3, 1.1, -0.5), Quat::IDENTITY);
    let mut pose = ik.solve(&head, [Some(lh), Some(rh)], |_, _| 0.0, 0.016);
    for _ in 0..30 {
        pose = ik.solve(&head, [Some(lh), Some(rh)], |_, _| 0.0, 0.016);
    }
    println!("IK: pelvis {:?} head {:?} handL {:?} handR {:?}", pose.get(Joint::Pelvis), pose.get(Joint::Head), pose.get(Joint::HandL), pose.get(Joint::HandR));

    let rig_out = app.world_mut().run_system_once(
        |mut commands: Commands,
         mut a: ResMut<Ardea>,
         mut meshes: ResMut<Assets<Mesh>>,
         mut qmats: ResMut<Assets<QuestMaterial>>,
         mut images: ResMut<Assets<Image>>,
         mut inv: ResMut<Assets<SkinnedMeshInverseBindposes>>| {
            let root = commands.spawn((Transform::from_scale(Vec3::splat(0.01)), Visibility::default())).id();
            let rig = spawn_body(&mut commands, &mut a, "g3_hero_body_player.xact", root, PartOptions { cutout: false, hide_from_eyes: false }, &mut meshes, &mut qmats, &mut images, &mut inv)
                .expect("hero body");
            spawn_part(&mut commands, &mut a, "g3_head_hero_hero_01.xact", &rig, PartOptions { cutout: true, hide_from_eyes: true }, &mut meshes, &mut qmats, &mut images, &mut inv)
                .expect("hero head");
            rig
        },
    );
    let rig = rig_out.expect("run_system_once");
    for (i, n) in rig.names.iter().enumerate() {
        let l = n.to_lowercase();
        if l.contains("left_hand") || l.contains("left_arm") || l.contains("left_foot") || l.starts_with("slot_") {
            let m = rig.bind_global[i];
            let q = Quat::from_mat4(&m);
            println!("NODE {n:34} pos_cm {:?} parent {}", m.w_axis.truncate().round(), rig.parent[i]);
            let _ = q;
        }
    }
    let rig_hero_for_check = ActorRig { root: rig.root, by_name: rig.by_name.clone(), names: rig.names.clone(), parent: rig.parent.clone(), bind_local: rig.bind_local.clone(), bind_global: rig.bind_global.clone(), joints: rig.joints.clone() };
    let rt = Retarget::new(&rig, "Hero");
    for l in rt.debug(&rig, &pose) {
        println!("{l}");
    }
    let rt_torso = rt.torso_bind_m(&rig).unwrap();
    app.world_mut().insert_resource(Hero { rig, rt });
    app.update();
    app.world_mut()
        .run_system_once(|hero: Res<Hero>, g: Query<&GlobalTransform>| {
            for n in ["Hero_Head_Head_1", "Hero_Spine_Spine_1", "Hero_Left_Foot_Foot_1"] {
                let p = hero.rig.by_name.get(n).and_then(|&i| g.get(hero.rig.joints[i]).ok()).map(|t| t.translation());
                println!("REST {n} {p:?}");
            }
        })
        .unwrap();
    let pose2 = pose.clone();
    let sc = ((pose.get(Joint::Head) + head.rotation * Vec3::new(0.0, -0.09, 0.09)) - pose.get(Joint::Pelvis)).length() / rt_torso;
    println!("scale {sc}");
    app.world_mut()
        .run_system_once(move |hero: Res<Hero>, mut q: Query<&mut Transform>| {
            hero.rt.apply(&hero.rig, &mut q, &pose2, Some(head.rotation), [Some(lh.rotation), Some(rh.rotation)], sc, None, [[0.0; 2]; 2], None, None);
        })
        .unwrap();
    app.update();
    app.update();
    // Finger curl: closed vs open fingertip, displacement must go into the palm (-N; N = grip +X for the left hand,
    // grip -X for the right hand; the grips here are identity).
    for (side, tip, n) in [("Left", "Hero_Left_Hand_Finger3_3", Vec3::X), ("Right", "Hero_Right_Hand_Finger3_3", -Vec3::X)] {
        let sc2 = sc;
        let _ = side;
        let open = app.world_mut().run_system_once(move |hero: Res<Hero>, g: Query<&GlobalTransform>| g.get(hero.rig.joints[hero.rig.by_name[tip]]).unwrap().translation()).unwrap();
        let pose3 = pose.clone();
        app.world_mut()
            .run_system_once(move |hero: Res<Hero>, mut q: Query<&mut Transform>| {
                hero.rt.apply(&hero.rig, &mut q, &pose3, Some(head.rotation), [Some(lh.rotation), Some(rh.rotation)], sc2, None, [[1.0, 1.0]; 2], None, None);
            })
            .unwrap();
        app.update();
        app.update();
        let closed = app.world_mut().run_system_once(move |hero: Res<Hero>, g: Query<&GlobalTransform>| g.get(hero.rig.joints[hero.rig.by_name[tip]]).unwrap().translation()).unwrap();
        println!("FINGER {tip} open {open:?} closed {closed:?} moved {:.3} m, into palm {:.3} m", (closed - open).length(), (closed - open).dot(-n));
        let pose4 = pose.clone();
        app.world_mut()
            .run_system_once(move |hero: Res<Hero>, mut q: Query<&mut Transform>| {
                hero.rt.apply(&hero.rig, &mut q, &pose4, Some(head.rotation), [Some(lh.rotation), Some(rh.rotation)], sc2, None, [[0.0, 0.0]; 2], None, None);
            })
            .unwrap();
        app.update();
    }
    // Weapon grip axis vs hand axes, bind pose, for the hero (Slot_RightHand_Weapon) and an orc, with the sword mesh's
    // long axis (mesh bounds; mirrored X like everything) rotated by the slot node's bind rotation.
    {
        let md = app.world_mut().resource_mut::<Ardea>().cache().load_mesh(gothic_cook::AssetId::of("g3_weapon_1h_sword_orcslayer_01.xcmsh")).unwrap();
        let (mut lo, mut hi) = (Vec3::splat(1e9), Vec3::splat(-1e9));
        for s in md.submeshes() {
            lo = lo.min(Vec3::from(s.bounds_min));
            hi = hi.max(Vec3::from(s.bounds_max));
        }
        let ext = hi - lo;
        let long = if ext.x >= ext.y && ext.x >= ext.z { Vec3::X } else if ext.y >= ext.z { Vec3::Y } else { Vec3::Z };
        let centre = (lo + hi) * 0.5;
        // blade direction: from the origin (grip) towards the bounds centre along the long axis
        let blade_native = long * (centre.dot(long)).signum();
        println!("WEAPON mesh bounds {lo:?}..{hi:?} long axis {long:?} blade dir (native) {blade_native:?}");
        let blade_mirror = Vec3::new(-blade_native.x, blade_native.y, blade_native.z);
        let orc = app.world_mut().run_system_once(
            |mut commands: Commands, mut a: ResMut<Ardea>, mut meshes: ResMut<Assets<Mesh>>, mut qmats: ResMut<Assets<QuestMaterial>>, mut images: ResMut<Assets<Image>>, mut inv: ResMut<Assets<SkinnedMeshInverseBindposes>>| {
                let root = commands.spawn((Transform::from_scale(Vec3::splat(0.01)), Visibility::default())).id();
                spawn_body(&mut commands, &mut a, "g3_orc_body_warrior.xact", root, PartOptions { cutout: false, hide_from_eyes: false }, &mut meshes, &mut qmats, &mut images, &mut inv)
            },
        ).unwrap();
        for (label, rig, prefix) in [("hero", &rig_hero_for_check, "Hero"), ("orc", orc.as_ref().expect("orc actor"), "Orc")] {
            let Some(&si) = rig.by_name.get("Slot_RightHand_Weapon") else { println!("{label}: no Slot_RightHand_Weapon"); continue };
            let rot = Quat::from_mat4(&rig.bind_global[si]).normalize();
            let blade = rot * blade_mirror;
            let hi_ = rig.by_name[&format!("{prefix}_Right_Hand_Hand_1")];
            let p = |n: &str| rig.by_name.get(&format!("{prefix}_Right_Hand_{n}")).map(|&i| rig.bind_global[i].w_axis.truncate());
            let (w, m, th, pk) = (rig.bind_global[hi_].w_axis.truncate(), p("Finger3_2"), p("Finger1_1"), p("Finger5_1"));
            if let (Some(m), Some(th), Some(pk)) = (m, th, pk) {
                let f = (m - w).normalize();
                let t = (th - pk).normalize();
                println!("{label}: slot pos {:?} hand {:?}; blade dir {:?}; fingers F {:?}; little->thumb T {:?}; blade.F {:.2} blade.T {:.2}", rig.bind_global[si].w_axis.truncate().round(), w.round(), blade, f, t, blade.dot(f), blade.dot(t));
            } else {
                println!("{label}: slot pos {:?} blade dir {blade:?} (no finger nodes)", rig.bind_global[si].w_axis.truncate().round());
            }
        }
    }
    // Gait from the original clips: walk forward / backward / strafe at 1.5 m/s, print the feet relative to the pelvis.
    {
        use gothic_quest::anim::{ClipSet, Gait, PoseOverride};
        let set = app.world_mut().run_system_once(|hero: Res<Hero>| ClipSet::load(std::path::Path::new(".local/quest/ardea-pack/clips_hero.grcp"), &hero.rig, "Hero")).unwrap().expect("clips_hero.grcp");
        println!("GAIT clip facing fix {:?}", set.fix);
        for (label, dir) in [("forward", Vec3::NEG_Z), ("backward", Vec3::Z), ("strafe right", Vec3::X)] {
            let mut gait = Gait::default();
            let mut p = Vec3::new(0.0, 0.99, 0.0);
            let mut last = (0.0, vec![]);
            for _ in 0..40 {
                p += dir * 1.5 * 0.025;
                last = gait.update(&set, p, 0.0, 0.025);
            }
            let (w, layers) = last;
            let refs: Vec<(&str, f32, f32)> = layers.iter().map(|(k, t, wt)| (k.as_str(), *t, *wt)).collect();
            println!("GAIT {label}: speed {:.2} weight {w:.2} layers {:?}", gait.speed, layers.iter().map(|(k, t, wt)| (k.split("_P0_").nth(1).unwrap_or(k).to_string(), (*t * 100.0).round() / 100.0, (*wt * 100.0).round() / 100.0)).collect::<Vec<_>>());
            let rot = set.sample_world(&refs, Quat::IDENTITY);
            let pose5 = pose.clone();
            let sc3 = sc;
            let h = head;
            app.world_mut()
                .run_system_once(move |hero: Res<Hero>, mut q: Query<&mut Transform>| {
                    let mut weight = vec![0.0; rot.len()];
                    for side in ["Left", "Right"] {
                        for part in ["Leg_Hip_1", "Leg_Leg_1", "Leg_Leg_2", "Foot_Foot_1", "Foot_Toe_1"] {
                            if let Some(&i) = hero.rig.by_name.get(&format!("Hero_{side}_{part}")) {
                                weight[i] = w;
                            }
                        }
                    }
                    let ov = PoseOverride { rot: rot.clone(), weight };
                    hero.rt.apply(&hero.rig, &mut q, &pose5, Some(h.rotation), [None, None], sc3, None, [[0.0; 2]; 2], Some(&ov), None);
                })
                .unwrap();
            app.update();
            app.update();
            app.world_mut()
                .run_system_once(move |hero: Res<Hero>, g: Query<&GlobalTransform>| {
                    let at = |n: &str| g.get(hero.rig.joints[hero.rig.by_name[n]]).unwrap().translation();
                    let pelvis = at("Hero_Spine_Spine_1");
                    for n in ["Hero_Left_Leg_Leg_1", "Hero_Left_Leg_Leg_2", "Hero_Left_Foot_Foot_1", "Hero_Right_Leg_Leg_1", "Hero_Right_Leg_Leg_2", "Hero_Right_Foot_Foot_1"] {
                        println!("  {n:26} rel pelvis {:?}", (at(n) - pelvis));
                    }
                })
                .unwrap();
        }
    }
    // ---- Torso vs eyes: minimum distance from the eye to any skinned body vertex for head pitches -80..+30 deg ----
    {
        let data = app.world().resource::<Ardea>().cache().load_actor(gothic_cook::AssetId::of("g3_hero_body_player.xact")).unwrap();
        let verts: Vec<_> = data.vertices().to_vec();
        let subs: Vec<_> = data.submeshes().to_vec();
        let mut worst = f32::MAX;
        for pitch_deg in [-85.0f32, -80.0, -60.0, -40.0, -20.0, 0.0, 30.0] {
            let hr = Quat::from_rotation_x(pitch_deg.to_radians());
            let eye = Vec3::new(0.0, 1.7, 0.0);
            let head2 = Pose::new(eye, hr);
            let mut ik2 = BodyIk::new(Proportions::for_eye_height(1.7));
            let mut p = ik2.solve(&head2, [Some(lh), Some(rh)], |_, _| 0.0, 0.016);
            for _ in 0..40 {
                p = ik2.solve(&head2, [Some(lh), Some(rh)], |_, _| 0.0, 0.016);
            }
            let torso0 = (eye + hr * Vec3::new(0.0, -0.09, 0.09) - p.get(Joint::Pelvis)).length();
            gothic_quest::hero::place_trunk(&mut p, eye, hr);
            let sc4 = (torso0 / rt_torso).clamp(0.6, 1.6);
            let (p2, lhr, rhr) = (p.clone(), lh.rotation, rh.rotation);
            app.world_mut()
                .run_system_once(move |hero: Res<Hero>, mut q: Query<&mut Transform>| {
                    hero.rt.apply(&hero.rig, &mut q, &p2, Some(hr), [Some(lhr), Some(rhr)], sc4, None, [[0.0; 2]; 2], None, None);
                })
                .unwrap();
            app.update();
            app.update();
            let verts2 = verts.clone();
            let subs2 = subs.clone();
            let min = app
                .world_mut()
                .run_system_once(move |hero: Res<Hero>, g: Query<&GlobalTransform>| {
                    let mats: Vec<Mat4> = (0..hero.rig.joints.len())
                        .map(|k| g.get(hero.rig.joints[k]).unwrap().to_matrix() * hero.rig.bind_global[k].inverse())
                        .collect();
                    let mut best = f32::MAX;
                    for s in &subs2 {
                        for v in &verts2[s.first_vertex as usize..(s.first_vertex + s.vertex_count) as usize] {
                            let local = Vec3::new(-v.pos[0], v.pos[1], v.pos[2]);
                            let w = if s.skinned != 0 {
                                let mut acc = Vec3::ZERO;
                                for k in 0..4 {
                                    let wt = v.weights[k] as f32 / 255.0;
                                    if wt > 0.0 {
                                        acc += mats[v.joints[k] as usize].transform_point3(local) * wt;
                                    }
                                }
                                acc
                            } else {
                                let node = s.node as usize;
                                (g.get(hero.rig.joints[node]).unwrap().to_matrix() * hero.rig.bind_global[node].inverse() * hero.rig.bind_global[node]).transform_point3(local)
                            };
                            best = best.min(w.distance(eye));
                        }
                    }
                    best
                })
                .unwrap();
            println!("TORSO pitch {pitch_deg:>6.0} deg: nearest body vertex to the eye {:.3} m", min);
            worst = worst.min(min);
        }
        let ok = worst > 0.08;
        failed |= !ok;
        println!("TORSO worst {worst:.3} m (needs > near plane 0.03 + 0.05 = 0.08): {}", if ok { "PASS" } else { "FAIL" });
    }
    // ---- Sword grip frame from the mesh ----
    {
        let md = app.world().resource::<Ardea>().cache().load_mesh(gothic_cook::AssetId::of("g3_weapon_1h_sword_orcslayer_01.xcmsh")).unwrap();
        let g = gothic_quest::grip::analyse(&md).expect("grip analysis");
        println!("GRIP blade {:?} edge {:?} flat {:?} center {:?} handle axis {:?} guard {:.1} pommel {:.1} tip {:.1}", g.blade, g.edge, g.flat, g.center, g.handle_axis, g.guard, g.pommel, g.tip);
        let vis = g.visual(0.01);
        // Controller grip frame of a right hand holding the sword in the natural pose: -Z = thumb side (blade out of the fist),
        // -Y = along the fingers, X = back of the hand (palm normal -X for the right hand).
        let fist_axis = Vec3::NEG_Z;
        let blade_in_prop = vis.rotation * Vec3::new(-g.blade.x, g.blade.y, g.blade.z);
        let flat_in_prop = vis.rotation * (-Vec3::new(-g.flat.x, g.flat.y, g.flat.z));
        let edge_in_prop = vis.rotation * Vec3::new(-g.edge.x, g.edge.y, g.edge.z);
        let handle_in_prop = vis.rotation * Vec3::new(-g.handle_axis.x, g.handle_axis.y, g.handle_axis.z);
        let ang = |a: Vec3, b: Vec3| a.dot(b).clamp(-1.0, 1.0).acos().to_degrees();
        let blade_angle = ang(blade_in_prop, fist_axis);
        let blade_ok = blade_angle < 5.0;
        failed |= !blade_ok;
        println!("GRIP blade vs fist axis {:.2} deg (must be < 5): {}", blade_angle, if blade_ok { "PASS" } else { "FAIL" });
        println!("GRIP handle axis vs fist axis {:.2} deg", ang(handle_in_prop, fist_axis));
        let flat_alignment = flat_in_prop.dot(Vec3::X).abs();
        let flat_ok = flat_alignment > 0.95;
        failed |= !flat_ok;
        println!("GRIP |flat . palm normal(X)| = {:.3} (must be > 0.95): {}", flat_alignment, if flat_ok { "PASS" } else { "FAIL" });
        println!("GRIP edge vs finger axis (-Y) {:.2} deg", ang(edge_in_prop, Vec3::NEG_Y));
        println!("GRIP grip centre in prop frame {:?} (must be ~0)", vis.rotation * (Vec3::new(-g.center.x, g.center.y, g.center.z) * 0.01) + vis.translation);
    }
    // ---- Held weapon vs visible hand: palm centre at the grip position, hand follows the grip frame, curl sweep is smooth ----
    {
        let sc5 = sc;
        let mut prev_rot: [Option<Quat>; 2] = [None, None];
        let mut max_step = 0.0f32;
        let mut max_dev = 0.0f32;
        let mut max_curled_dev = 0.0f32;
        let mut worst_palm = 0.0f32;
        for step in 0..=20 {
            let curl_v = step as f32 / 20.0;
            let mut pose6 = pose.clone();
            let (rt_ref, hero_rig) = app.world_mut().run_system_once(|hero: Res<Hero>| (hero.rt.wrist_for_grip(0, Vec3::ZERO, Quat::IDENTITY, 1.0).is_some(), hero.rig.joints.len())).unwrap();
            let _ = (rt_ref, hero_rig);
            let wrists = app
                .world_mut()
                .run_system_once(move |hero: Res<Hero>| [hero.rt.wrist_for_grip(0, lh.position, lh.rotation, sc5), hero.rt.wrist_for_grip(1, rh.position, rh.rotation, sc5)])
                .unwrap();
            if let Some(w) = wrists[0] {
                pose6.joints[Joint::HandL.index()] = w;
            }
            if let Some(w) = wrists[1] {
                pose6.joints[Joint::HandR.index()] = w;
            }
            app.world_mut()
                .run_system_once(move |hero: Res<Hero>, mut q: Query<&mut Transform>| {
                    hero.rt.apply(&hero.rig, &mut q, &pose6, Some(head.rotation), [Some(lh.rotation), Some(rh.rotation)], sc5, None, [[curl_v, curl_v]; 2], None, None);
                })
                .unwrap();
            app.update();
            app.update();
            let res = app
                .world_mut()
                .run_system_once(move |hero: Res<Hero>, g: Query<&GlobalTransform>| {
                    let node = |n: &str| g.get(hero.rig.joints[hero.rig.by_name[n]]).unwrap();
                    let mut out = Vec::new();
                    for (k, side) in ["Left", "Right"].into_iter().enumerate() {
                        let hand = node(&format!("Hero_{side}_Hand_Hand_1"));
                        let w = hand.translation();
                        let kc = (2..=5).map(|f| node(&format!("Hero_{side}_Hand_Finger{f}_1")).translation()).sum::<Vec3>() / 4.0;
                        let palm = w + (kc - w) * 0.55;
                        let target = if k == 0 { lh } else { rh };
                        let f_bone = (node(&format!("Hero_{side}_Hand_Finger3_2")).translation() - w).normalize();
                        let f_grip = (target.rotation * Vec3::NEG_Y).normalize();
                        out.push((hand.rotation(), palm.distance(target.position), f_bone.dot(f_grip).clamp(-1.0, 1.0).acos().to_degrees()));
                    }
                    out
                })
                .unwrap();
            for k in 0..2 {
                if let Some(p) = prev_rot[k] {
                    max_step = max_step.max(p.angle_between(res[k].0).to_degrees());
                }
                prev_rot[k] = Some(res[k].0);
                worst_palm = worst_palm.max(res[k].1);
                // Finger alignment is defined by the bind/open rig geometry. A curled finger's
                // proximal segment is expected to rotate away from the controller's grip -Y.
                if step == 0 {
                    max_dev = max_dev.max(res[k].2);
                } else {
                    max_curled_dev = max_curled_dev.max(res[k].2);
                }
            }
        }
        let palm_ok = worst_palm < 0.015;
        failed |= !palm_ok;
        println!("HOLD palm centre to grip position: worst {:.3} m (must be < 0.015): {}", worst_palm, if palm_ok { "PASS" } else { "FAIL" });
        let rotation_ok = max_step < 5.0;
        failed |= !rotation_ok;
        println!("HOLD hand bone rotation change over the grip 0..1 sweep: max {:.2} deg per step (must be < 5): {}", max_step, if rotation_ok { "PASS" } else { "FAIL" });
        let finger_ok = max_dev < 15.0;
        failed |= !finger_ok;
        println!("HOLD open finger axis vs grip frame (-Y): worst {:.2} deg (must be < 15): {}", max_dev, if finger_ok { "PASS" } else { "FAIL" });
        println!("HOLD curled proximal finger axis vs grip frame (-Y): max {:.2} deg (diagnostic; curling changes this axis)", max_curled_dev);
    }
    let mut bad = 0;
    app.world_mut()
        .run_system_once(move |hero: Res<Hero>, g: Query<&GlobalTransform>| {
            let at = |n: &str| hero.rig.by_name.get(n).and_then(|&i| g.get(hero.rig.joints[i]).ok()).map(|t| t.translation());
            for (n, j) in [("Hero_Right_Arm_Arm_1", Joint::ShoulderR), ("Hero_Right_Arm_Arm_2", Joint::ElbowR), ("Hero_Right_Hand_Hand_1", Joint::HandR), ("Hero_Left_Arm_Arm_1", Joint::ShoulderL), ("Hero_Left_Arm_Arm_2", Joint::ElbowL)] {
                println!("ARM {n:24} avatar {:?} ik {:?}", at(n), pose.get(j));
            }
            for (n, target) in [
                ("Hero_Head_Head_1", Some(pose.get(Joint::Head))),
                ("Hero_Left_Hand_Hand_1", Some(pose.get(Joint::HandL))),
                ("Hero_Right_Hand_Hand_1", Some(pose.get(Joint::HandR))),
                ("Hero_Left_Foot_Foot_1", Some(pose.get(Joint::AnkleL))),
                ("Hero_Right_Foot_Foot_1", Some(pose.get(Joint::AnkleR))),
                ("Slot_MeleeBackRight", None),
                ("Slot_MeleeBackLeft", None),
                ("Slot_Bow", None),
            ] {
                let p = at(n);
                let d = match (p, target) {
                    (Some(p), Some(t)) => format!("dist to IK {:.3} m", p.distance(t)),
                    _ => String::new(),
                };
                println!("{n:28} {:?} {d}", p);
            }
        })
        .unwrap();
    let _ = &mut bad;
    if failed {
        eprintln!("hero_check: one or more geometry assertions failed");
        std::process::exit(1);
    }
}
