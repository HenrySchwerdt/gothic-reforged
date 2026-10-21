//! `LibrarySource` against synthetic archives (no game data): a tiny body and head actor and
//! one DXT1 texture, written into G3V0 archives in a temporary directory. The streamed bundle
//! and the committed character must match what the blocking `ActorLibrary` /
//! `ActorSpawner::spawn` path produces from the same files.
use bevy::asset::AssetPlugin;
use bevy::ecs::system::RunSystemOnce;
use bevy::mesh::VertexAttributeValues;
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use gothic_animation::runtime::{CharacterComposition, CharacterJoints, PartMeshes};
use gothic_animation::stream::AttachRequest;
use gothic_animation::{
    ActorLibrary, ActorSpawner, ActorStreamPlugin, ActorStreamer, Animator, AssetStores,
    AttachSpec, BuildRequest, LibrarySource, Locomotion, LocomotionInput, NpcSpec, PendingActor,
    Stance, StreamConfig, WeaponSpec,
};
use gothic_formats::synth::{Target, structured};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

// ---- synthetic files -------------------------------------------------------------------

fn u32b(b: &mut Vec<u8>, n: u32) {
    b.extend(n.to_le_bytes());
}
fn f32s(b: &mut Vec<u8>, v: &[f32]) {
    for x in v {
        b.extend(x.to_le_bytes());
    }
}
fn string(b: &mut Vec<u8>, s: &str) {
    u32b(b, s.len() as u32);
    b.extend(s.as_bytes());
}
fn chunk(out: &mut Vec<u8>, id: u32, version: u32, data: &[u8]) {
    u32b(out, id);
    u32b(out, data.len() as u32);
    u32b(out, version);
    out.extend(data);
}
fn node(name: &str, parent: &str, pos: [f32; 3]) -> Vec<u8> {
    let mut d = Vec::new();
    f32s(&mut d, &pos);
    f32s(&mut d, &[0., 0., 0., 1.]);
    f32s(&mut d, &[0., 0., 0., 1.]);
    f32s(&mut d, &[1., 1., 1.]);
    f32s(&mut d, &[0., 0., 0.]);
    string(&mut d, name);
    string(&mut d, parent);
    d
}
fn material(name: &str) -> Vec<u8> {
    let mut d = Vec::new();
    f32s(&mut d, &[0.; 3]);
    f32s(&mut d, &[0.75; 3]);
    f32s(&mut d, &[1.; 3]);
    f32s(&mut d, &[0.; 3]);
    f32s(&mut d, &[25., 0., 1., 1.5]);
    d.extend([0, 0, 0x46, 0]);
    string(&mut d, name);
    u32b(&mut d, 0);
    d
}
fn layer(material: u16, texture: &str) -> Vec<u8> {
    let mut d = vec![1, 0xf7];
    d.extend(material.to_le_bytes());
    f32s(&mut d, &[1., 0., 0., 1., 1., 0.]);
    string(&mut d, texture);
    d
}
/// One triangle on node 1, material 1.
fn mesh() -> Vec<u8> {
    let mut d = Vec::new();
    for n in [1u32, 3, 3, 3, 1, 1, 0xc9_00] {
        u32b(&mut d, n);
    }
    d.extend([1, 1, 0, 0]);
    u32b(&mut d, 3);
    u32b(&mut d, 3);
    for (i, p) in [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]]
        .iter()
        .enumerate()
    {
        u32b(&mut d, i as u32);
        f32s(&mut d, p);
        f32s(&mut d, &[0., 0., 1.]);
        f32s(&mut d, &[i as f32, 0.]);
    }
    for i in 0..3 {
        u32b(&mut d, i);
    }
    d
}
/// Weights of the three vertices on nodes 0 and 1.
fn skin() -> Vec<u8> {
    let mut d = Vec::new();
    u32b(&mut d, 1);
    for bones in [&[(0u16, 1.0f32)][..], &[(0, 0.25), (1, 0.75)], &[(1, 1.0)]] {
        d.push(bones.len() as u8);
        for (b, w) in bones {
            d.extend(b.to_le_bytes());
            d.extend([0xd3, 0x13]);
            d.extend(w.to_le_bytes());
        }
    }
    d
}
/// A `.xact` with nodes `Root`, `Mesh` and `extra` (name, parent), one skinned triangle
/// and a material whose diffuse layer is `texture`.
fn xact(extra: (&str, &str), texture: &str) -> Vec<u8> {
    let mut chunks = Vec::new();
    chunk(&mut chunks, 0, 3, &node("Root", "", [0., 0., 0.]));
    chunk(&mut chunks, 0, 3, &node("Mesh", "Root", [0., 2., 0.]));
    chunk(&mut chunks, 0, 3, &node(extra.0, extra.1, [3., 1., 0.5]));
    chunk(&mut chunks, 6, 5, &material("EMFX Default"));
    chunk(&mut chunks, 6, 5, &material("Skin"));
    chunk(&mut chunks, 7, 4, &layer(1, texture));
    chunk(&mut chunks, 3, 3, &mesh());
    chunk(&mut chunks, 4, 1, &skin());
    let mut body = vec![0x36, 0, 0, 0, 0, 0];
    body.extend(b"gena");
    body.extend(4u16.to_le_bytes());
    u32b(&mut body, (2 + chunks.len() + 4) as u32);
    body.extend(b"FXA ");
    body.extend([1, 1]);
    body.extend(chunks);
    body.extend([0; 4]);
    let mut out = b"GENOMFLE".to_vec();
    out.extend(1u16.to_le_bytes());
    out.extend(((14 + body.len()) as u32).to_le_bytes());
    out.extend(&body);
    out.extend([0xef, 0xbe, 0xad, 0xde, 1]);
    u32b(&mut out, 1);
    let name = b"Skin.xshmat";
    out.extend((name.len() as u16).to_le_bytes());
    out.extend(name);
    out
}
/// A 4x4 DXT1 `.ximg` (one mip).
fn ximg() -> Vec<u8> {
    structured(
        Target::Ximg,
        &[
            0, 3, 3, 0, 0, 0x1f, 0xf8, 0xe0, 0x07, 0x1b, 0xe4, 0x1b, 0xe4,
        ],
    )
}
/// A G3V0 archive of stored members.
fn pak(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let s = |out: &mut Vec<u8>, v: &str| {
        u32b(out, v.len() as u32);
        out.extend(v.as_bytes());
        if !v.is_empty() {
            out.push(0);
        }
    };
    let data_start = 48u64;
    let data_len: u64 = files.iter().map(|f| f.1.len() as u64).sum();
    let table = data_start + data_len;
    let mut out = Vec::new();
    u32b(&mut out, 0);
    u32b(&mut out, 0x3056_3347);
    out.extend([0u8; 16]);
    for _ in 0..3 {
        out.extend(table.to_le_bytes());
    }
    for (_, bytes) in files {
        out.extend(bytes);
    }
    out.extend([0u8; 32]);
    u32b(&mut out, 0x10);
    s(&mut out, "root");
    u32b(&mut out, files.len() as u32);
    let mut offset = data_start;
    for (name, bytes) in files {
        out.extend([0u8; 32]);
        u32b(&mut out, 0);
        out.extend(offset.to_le_bytes());
        out.extend((bytes.len() as u64).to_le_bytes());
        out.extend((bytes.len() as u64).to_le_bytes());
        u32b(&mut out, 0);
        u32b(&mut out, 0);
        s(&mut out, name);
        s(&mut out, "");
        offset += bytes.len() as u64;
    }
    u32b(&mut out, 0);
    out
}

/// Data directory with `Syn_Body` (slot `Slot_RightHand_Weapon`, texture present) and
/// `Syn_Head` (slot `Slot_Hair` only the head has, texture missing).
fn data_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gr-library-source-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let write = |name: &str, bytes: Vec<u8>| std::fs::write(dir.join(name), bytes).unwrap();
    write(
        "_compiledAnimation.pak",
        pak(&[
            (
                "Syn_Body.xact",
                xact(("Slot_RightHand_Weapon", "Root"), "Tex_Body"),
            ),
            ("Syn_Head.xact", xact(("Slot_Hair", "Mesh"), "Tex_Head")),
        ]),
    );
    write("_compiledMaterial.pak", pak(&[]));
    write("_compiledImage.pak", pak(&[("Tex_Body.ximg", ximg())]));
    write("_compiledMesh.pak", pak(&[]));
    dir
}

const PARTS: [&str; 2] = ["Syn_Body", "Syn_Head"];

// ---- comparisons -------------------------------------------------------------------------

fn attr(mesh: &Mesh, id: bevy::mesh::MeshVertexAttribute) -> String {
    format!("{:?}", mesh.attribute(id))
}

fn assert_same_mesh(a: &Mesh, b: &Mesh) {
    for id in [
        Mesh::ATTRIBUTE_POSITION,
        Mesh::ATTRIBUTE_NORMAL,
        Mesh::ATTRIBUTE_UV_0,
        Mesh::ATTRIBUTE_JOINT_INDEX,
        Mesh::ATTRIBUTE_JOINT_WEIGHT,
    ] {
        assert_eq!(attr(a, id), attr(b, id), "attribute {}", id.name);
    }
    assert_eq!(format!("{:?}", a.indices()), format!("{:?}", b.indices()));
}

fn assert_same_material(a: &StandardMaterial, b: &StandardMaterial) {
    assert_eq!(a.base_color, b.base_color);
    assert_eq!(a.alpha_mode, b.alpha_mode);
    assert_eq!(a.perceptual_roughness, b.perceptual_roughness);
    assert_eq!(a.reflectance, b.reflectance);
    assert_eq!(a.cull_mode, b.cull_mode);
}

#[test]
fn bundle_matches_the_blocking_composition() {
    let dir = data_dir("bundle");
    let mut blocking = ActorLibrary::open(&dir).unwrap();
    let mut meshes = Assets::<Mesh>::default();
    let mut materials = Assets::<StandardMaterial>::default();
    let mut images = Assets::<Image>::default();
    let mut bindposes = Assets::<SkinnedMeshInverseBindposes>::default();
    let composition = {
        let mut stores = AssetStores {
            meshes: &mut meshes,
            materials: &mut materials,
            images: &mut images,
            bindposes: &mut bindposes,
        };
        blocking.composition(&PARTS, &mut stores).unwrap()
    };
    let mut cpu = ActorLibrary::open(&dir).unwrap();
    let bundle = cpu.build_bundle(&BuildRequest::new(PARTS)).unwrap();

    assert_eq!(bundle.key, "Syn_Body+Syn_Head");
    assert_eq!(bundle.rig.names, composition.rig.names);
    assert_eq!(bundle.rig.family, composition.rig.family);
    assert_eq!(&*bundle.joint_nodes, &*composition.joint_nodes);
    assert_eq!(bundle.plan.len(), composition.plan.len());
    assert_eq!(bundle.joint_by_name, composition.joint_by_name);
    assert_eq!(bundle.facing_fix, composition.facing_fix);
    // The head-only slot hangs off a shared joint at its bind offset.
    assert_eq!(bundle.slot_mounts.len(), 1);
    let (j, offset) = bundle.slot_mounts["slot_hair"];
    assert_eq!((j, offset), composition.slot_mounts["slot_hair"]);
    // No motions in the archive: no idle, locomotion on, nothing else to play.
    assert!(bundle.locomotion && bundle.motion.is_none() && bundle.idle.is_empty());

    assert_eq!(bundle.parts.len(), composition.parts.len());
    for ((cpu_part, part), joints) in bundle
        .parts
        .iter()
        .zip(&composition.parts)
        .zip(&composition.part_joints)
    {
        assert_eq!(cpu_part.name, part.name);
        assert_eq!(cpu_part.rig.names, part.rig.names);
        assert_eq!(cpu_part.joint_rig_nodes, part.joint_nodes);
        assert_eq!(&cpu_part.joints, joints);
        assert_eq!(cpu_part.aabb, part.aabb);
        assert_eq!(
            &cpu_part.inverse_bindposes[..],
            &bindposes.get(&part.inverse_bindposes).unwrap()[..]
        );
        assert_eq!(cpu_part.meshes.len(), part.meshes.len());
        for (cm, pm) in cpu_part.meshes.iter().zip(&part.meshes) {
            assert_same_mesh(&cm.mesh, meshes.get(&pm.mesh).unwrap());
            let material = materials.get(&pm.material).unwrap();
            assert_same_material(&cm.material.material, material);
            match (&cm.material.texture, &material.base_color_texture) {
                (Some((key, image)), Some(handle)) => {
                    assert_eq!(key, "tex_body.ximg");
                    let blocking_image = images.get(handle).unwrap();
                    assert_eq!(image.data, blocking_image.data);
                    assert_eq!(image.texture_descriptor, blocking_image.texture_descriptor);
                }
                (None, None) => {}
                other => panic!("texture differs: {:?}", (other.0.is_some(), other.1)),
            }
        }
    }
    // Body textured, head without its (missing) texture.
    assert!(bundle.parts[0].meshes[0].material.texture.is_some());
    assert!(bundle.parts[1].meshes[0].material.texture.is_none());
    // Left-handed fix: the authored vertex (1, 0, 0) is mirrored to (-1, 0, 0).
    let Some(VertexAttributeValues::Float32x3(p)) = bundle.parts[0].meshes[0]
        .mesh
        .attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        panic!("positions");
    };
    assert_eq!(p[1], [-1.0, 0.0, 0.0]);
}

fn app(source: LibrarySource, library: Option<ActorLibrary>) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Mesh>()
        .init_asset::<Image>()
        .init_asset::<StandardMaterial>()
        .init_asset::<SkinnedMeshInverseBindposes>()
        .add_plugins(ActorStreamPlugin)
        .insert_resource(ActorStreamer::new(
            Arc::new(source),
            StreamConfig {
                workers: 2,
                ..Default::default()
            },
        ));
    if let Some(library) = library {
        app.insert_resource(library);
    }
    app
}

fn request(app: &mut App, req: BuildRequest) -> Entity {
    app.world_mut()
        .resource_scope(|world, mut s: Mut<ActorStreamer>| {
            let mut commands = world.commands();
            let e = s.request(&mut commands, req, Transform::default(), Stance::OneHand);
            world.flush();
            e
        })
}

fn run_until_idle(app: &mut App) {
    for _ in 0..2000 {
        app.update();
        if app.world().resource::<ActorStreamer>().is_idle() {
            return;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    panic!("streamer did not finish");
}

fn mesh_children(world: &World, root: Entity) -> Vec<Entity> {
    world
        .get::<Children>(root)
        .map(|c| {
            c.iter()
                .filter(|e| world.get::<Mesh3d>(*e).is_some())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn streamed_character_matches_actor_spawner() {
    let dir = data_dir("spawn");
    let spec = NpcSpec::new("Syn_Body")
        .with_part("Syn_Head")
        .with_stance(Stance::OneHand)
        .with_start_time(0.5);
    let mut app = app(
        LibrarySource::open(&dir),
        Some(ActorLibrary::open(&dir).unwrap()),
    );
    let blocking_spec = spec.clone();
    let blocking = app
        .world_mut()
        .run_system_once(move |mut spawner: ActorSpawner| {
            spawner.spawn(&blocking_spec, Transform::default()).unwrap()
        })
        .unwrap();
    let streamed = request(&mut app, BuildRequest::from_spec(&spec).unwrap());
    run_until_idle(&mut app);
    let world = app.world();
    assert!(world.get::<PendingActor>(streamed).is_none());
    for e in [blocking, streamed] {
        assert!(world.get::<Animator>(e).is_some());
        assert!(world.get::<Locomotion>(e).is_some());
        assert_eq!(
            world.get::<LocomotionInput>(e).unwrap().stance,
            Stance::OneHand
        );
    }
    let a = &world.get::<CharacterComposition>(blocking).unwrap().0;
    let b = &world.get::<CharacterComposition>(streamed).unwrap().0;
    assert_eq!(&*a.joint_nodes, &*b.joint_nodes);
    assert_eq!(a.part_joints, b.part_joints);
    assert_eq!(a.slot_mounts, b.slot_mounts);
    assert_eq!(a.joint_by_name, b.joint_by_name);
    let parts = |e| {
        world
            .get::<PartMeshes>(e)
            .unwrap()
            .0
            .iter()
            .map(|(n, m)| (n.clone(), m.len()))
            .collect::<Vec<_>>()
    };
    assert_eq!(parts(blocking), parts(streamed));
    assert_eq!(
        world.get::<CharacterJoints>(blocking).unwrap().0.len(),
        world.get::<CharacterJoints>(streamed).unwrap().0.len()
    );
    let meshes = world.resource::<Assets<Mesh>>();
    let ma = mesh_children(world, blocking);
    let mb = mesh_children(world, streamed);
    assert_eq!(ma.len(), mb.len());
    for (x, y) in ma.iter().zip(&mb) {
        let mx = meshes.get(&world.get::<Mesh3d>(*x).unwrap().0).unwrap();
        let my = meshes.get(&world.get::<Mesh3d>(*y).unwrap().0).unwrap();
        assert_same_mesh(mx, my);
    }
}

#[test]
fn failures_match_the_blocking_errors() {
    let dir = data_dir("errors");
    let mut lib = ActorLibrary::open(&dir).unwrap();
    // Unknown actor.
    let e = lib
        .build_bundle(&BuildRequest::new(["Nobody_Here"]))
        .err()
        .unwrap();
    assert!(format!("{e:#}").contains("no .xact actor matches"), "{e:#}");
    // No parts.
    assert!(
        lib.build_bundle(&BuildRequest::new(Vec::<String>::new()))
            .is_err()
    );
    // A weapon whose mesh the archive lacks.
    let mut req = BuildRequest::new(PARTS);
    req.attachments.push(AttachRequest {
        mesh: "G3_Weapon_Missing".into(),
        slot: "Slot_RightHand_Weapon".into(),
        holster_slot: Some("Slot_Back".into()),
    });
    let e = lib.build_bundle(&req).err().unwrap();
    assert!(format!("{e:#}").contains("no .xcmsh mesh"), "{e:#}");

    // The streamer reports it and drops the request; the bare look still spawns.
    let mut app = app(LibrarySource::open(&dir), None);
    let broken = request(&mut app, req.clone());
    let bare = request(&mut app, BuildRequest::new(PARTS));
    run_until_idle(&mut app);
    let world = app.world();
    assert!(world.get::<Animator>(broken).is_none());
    assert!(world.get::<Animator>(bare).is_some());
    let s = world.resource::<ActorStreamer>();
    assert!(s.failure(&req.key()).unwrap().contains("no .xcmsh mesh"));
}

#[test]
fn unopenable_archives_fail_the_build() {
    let missing = Path::new("/nonexistent/gothic-reforged-data");
    let mut app = app(LibrarySource::open(missing), None);
    let req = BuildRequest::new(PARTS);
    let e = request(&mut app, req.clone());
    run_until_idle(&mut app);
    let s = app.world().resource::<ActorStreamer>();
    assert!(
        s.failure(&req.key())
            .unwrap()
            .contains("opening the actor archives")
    );
    assert!(app.world().get::<Animator>(e).is_none());
}

#[test]
fn pooled_libraries_are_reused() {
    let dir = data_dir("pool");
    let source = LibrarySource::open(&dir);
    use gothic_animation::BuildSource;
    for _ in 0..3 {
        source.build(&BuildRequest::new(PARTS)).unwrap();
    }
    assert_eq!(source.opened(), 1);
}

#[test]
fn requests_from_specs() {
    let spec = NpcSpec::new("Body")
        .with_part("Head@M2")
        .with_weapon(WeaponSpec::one_hand_sword("Sword"))
        .with_attachment(AttachSpec {
            mesh: "Hair".into(),
            switch: 3,
            slot: "Slot_Hair".into(),
        })
        .with_attachment(AttachSpec {
            mesh: "Beard".into(),
            switch: -1,
            slot: "Slot_Beard".into(),
        })
        .with_start_time(1.25);
    let req = BuildRequest::from_spec(&spec).unwrap();
    assert_eq!(req.parts, ["Body", "Head@M2"]);
    assert_eq!(req.start_time, 1.25);
    assert_eq!(
        req.attachments,
        [
            AttachRequest {
                mesh: "Sword".into(),
                slot: "Slot_RightHand_Weapon".into(),
                holster_slot: Some("Slot_MeleeBackRight".into()),
            },
            AttachRequest {
                mesh: "Hair@M3".into(),
                slot: "Slot_Hair".into(),
                holster_slot: None,
            },
            AttachRequest {
                mesh: "Beard".into(),
                slot: "Slot_Beard".into(),
                holster_slot: None,
            },
        ]
    );
    // The start time desynchronises characters but does not change the look.
    let other = BuildRequest::from_spec(&spec.clone().with_start_time(0.0)).unwrap();
    assert_eq!(req.key(), other.key());
    // Weapon offsets are refused rather than dropped.
    let mut offset = WeaponSpec::one_hand_sword("Sword");
    offset.offset = Transform::from_xyz(1.0, 0.0, 0.0);
    assert!(BuildRequest::from_spec(&NpcSpec::new("Body").with_weapon(offset)).is_err());
}
