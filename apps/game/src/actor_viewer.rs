//! Development viewer for skinned `.xact` actors driven by `.xmot` motions.
//!
//! `gothic-reforged --actor Hero [--motion walk] [--smoke]`: loads the actor(s) from
//! `_compiledAnimation.pak`, resolves textures through `_compiledMaterial.pak` and
//! `_compiledImage.pak`, builds a Bevy joint hierarchy with `SkinnedMesh`, and samples the
//! motion's linear key tracks each frame. Explicit archives only, no patch overlay.
use anyhow::{Result, anyhow, ensure};
use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::NoFrustumCulling,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll},
    mesh::{
        Indices, PrimitiveTopology, VertexAttributeValues,
        skinning::{SkinnedMesh, SkinnedMeshInverseBindposes},
    },
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    window::WindowResolution,
};
use gothic_formats::{PakArchive, xact::Actor, ximg::Ximg, xmot::Motion, xshmat::Xshmat};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

const DEFAULT_DATA_DIR: &str = r"D:\SteamLibrary\steamapps\common\Gothic 3\Data";

struct ImageData {
    width: u32,
    height: u32,
    fourcc: [u8; 4],
    mips: Vec<Vec<u8>>,
}

struct ActorData {
    name: String,
    actor: Actor,
    /// Per material index: image index, alpha mode hint.
    textures: Vec<Option<usize>>,
    blend: Vec<Option<(u32, u8)>>,
}

#[derive(Resource)]
struct Scene {
    actors: Vec<ActorData>,
    images: Vec<ImageData>,
    motion: Option<(String, Motion)>,
    label: String,
    yaw_degrees: f32,
    in_place: bool,
}

#[derive(Resource)]
struct Smoke {
    enabled: bool,
    path: String,
    second_path: String,
    times: [f32; 2],
    frame: u32,
}

/// Marks a joint entity: which actor and node it represents.
#[derive(Component)]
struct Joint {
    actor: usize,
    node: usize,
}

#[derive(Resource)]
struct Playback {
    time: f32,
    paused: bool,
    /// Per actor, per node: index of the motion node with the same name.
    map: Vec<Vec<Option<usize>>>,
    /// Bind-pose local TRS per actor and node.
    bind: Vec<Vec<(Vec3, Quat, Vec3)>>,
    /// True for nodes without parent (root motion is flattened when in place).
    root: Vec<Vec<bool>>,
    /// Constant matrix applied before the motion's local transform: motions are expressed
    /// relative to the nearest *animated* ancestor, so the bind-pose transform of any helper
    /// nodes (`*_ROOT`, `*_END`) between them must be undone.
    prefix: Vec<Vec<Mat4>>,
}

#[derive(Component)]
struct Orbit {
    target: Vec3,
    distance: f32,
    yaw: f32,
    pitch: f32,
}

pub fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let flag = |name: &str| -> Option<&str> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .filter(|s| !s.starts_with("--"))
            .map(String::as_str)
    };
    let data_dir = PathBuf::from(flag("--data-dir").unwrap_or(DEFAULT_DATA_DIR));
    let actor_arg = flag("--actor").ok_or_else(|| anyhow!("--actor requires a name (try Hero)"))?;
    let smoke = args.iter().any(|a| a == "--smoke");
    let scene = load(
        &data_dir,
        actor_arg,
        flag("--motion"),
        flag("--yaw").map(str::parse).transpose()?.unwrap_or(0.0),
        args.iter().any(|a| a == "--root-motion"),
    )?;
    let screenshot = flag("--screenshot")
        .unwrap_or(".local/actor-smoke.png")
        .to_string();
    let second_path = match screenshot.strip_suffix(".png") {
        Some(stem) => format!("{stem}-2.png"),
        None => format!("{screenshot}-2"),
    };
    let duration = scene.motion.as_ref().map_or(0.0, |(_, m)| m.duration);
    let time_a: f32 = flag("--time-a").map(str::parse).transpose()?.unwrap_or(0.0);
    let time_b: f32 = flag("--time-b")
        .map(str::parse)
        .transpose()?
        .unwrap_or(duration * 0.5);
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Gothic Reforged / actor viewer".into(),
                resolution: WindowResolution::new(1280, 720),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(scene)
        .insert_resource(Smoke {
            enabled: smoke,
            path: screenshot,
            second_path,
            times: [time_a, time_b],
            frame: 0,
        })
        .insert_resource(ClearColor(Color::srgb(0.07, 0.08, 0.1)))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (animate, orbit, smoke_capture, update_title).chain(),
        )
        .run();
    Ok(())
}

fn find_member(pak: &PakArchive, wanted: &str, extension: &str) -> Result<usize> {
    let wanted_lower = wanted.to_lowercase();
    let with_ext = if wanted_lower.ends_with(extension) {
        wanted_lower.clone()
    } else {
        format!("{wanted_lower}{extension}")
    };
    let live = |e: &&gothic_formats::PakEntry| !e.deleted() && !e.payloadless();
    if let Some(i) = pak
        .entries
        .iter()
        .position(|e| live(&e) && e.path.to_lowercase() == with_ext)
    {
        return Ok(i);
    }
    let mut hits: Vec<usize> = pak
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            live(e) && {
                let p = e.path.to_lowercase();
                p.ends_with(extension) && p.contains(&wanted_lower)
            }
        })
        .map(|(i, _)| i)
        .collect();
    hits.sort_by_key(|&i| pak.entries[i].path.len());
    let first = *hits
        .first()
        .ok_or_else(|| anyhow!("no {extension} member matches {wanted:?}"))?;
    if hits.len() > 1 {
        eprintln!(
            "{wanted:?} matched {} members; using {} (shortest name)",
            hits.len(),
            pak.entries[first].path
        );
    }
    Ok(first)
}

fn load(
    data_dir: &Path,
    actor_arg: &str,
    motion_arg: Option<&str>,
    yaw_degrees: f32,
    root_motion: bool,
) -> Result<Scene> {
    let mut animation = PakArchive::open(data_dir.join("_compiledAnimation.pak"))?;
    let mut material_pak = PakArchive::open(data_dir.join("_compiledMaterial.pak"))?;
    let mut image_pak = PakArchive::open(data_dir.join("_compiledImage.pak"))?;
    // Unique-basename lookup, first occurrence wins (the importer reports ambiguity).
    let basename = |p: &str| p.rsplit('/').next().unwrap_or(p).to_lowercase();
    let materials_index: HashMap<String, usize> = material_pak
        .entries
        .iter()
        .enumerate()
        .rev()
        .map(|(i, e)| (basename(&e.path), i))
        .collect();
    let images_index: HashMap<String, usize> = image_pak
        .entries
        .iter()
        .enumerate()
        .rev()
        .map(|(i, e)| (basename(&e.path), i))
        .collect();

    // "Hero" is a convenience alias for the player body plus head.
    let names: Vec<String> = if actor_arg.eq_ignore_ascii_case("hero") {
        vec!["G3_Hero_Body_Player".into(), "G3_Hero_Head_Player".into()]
    } else {
        actor_arg.split(',').map(str::to_string).collect()
    };
    let mut images: Vec<ImageData> = Vec::new();
    let mut image_cache: HashMap<String, Option<usize>> = HashMap::new();
    let mut actors = Vec::new();
    for name in &names {
        let at = find_member(&animation, name, ".xact")?;
        let path = animation.entries[at].path.clone();
        let actor = Actor::parse(&animation.read_payload(at)?)
            .map_err(|e| e.context(format!("parsing {path}")))?;
        let mut textures = vec![None; actor.materials.len()];
        let mut blend = vec![None; actor.materials.len()];
        for (index, material) in actor.materials.iter().enumerate().skip(1) {
            let mut stem: Option<String> = None;
            let listed = actor
                .material_file(index)
                .map(str::to_string)
                .unwrap_or_else(|| format!("{}.xshmat", material.name.trim_end_matches('|')));
            // Candidate shader materials. The listed file can be a generic Maya name
            // (`lambert2SG.xshmat` exists in the archive but belongs to another model), so the
            // layer's texture name (`<texture>_A.xshmat`) is tried first.
            let mut candidates = Vec::new();
            if let Some(texture) = material.diffuse_texture() {
                candidates.push(format!("{texture}_A.xshmat"));
                candidates.push(format!("{texture}.xshmat"));
            }
            candidates.push(listed.clone());
            let file = candidates
                .iter()
                .find(|c| materials_index.contains_key(&c.to_lowercase()))
                .cloned()
                .unwrap_or(listed);
            if let Some(&m) = materials_index.get(&file.to_lowercase()) {
                let parsed = Xshmat::parse(&material_pak.read_payload(m)?)?;
                if let Some(d) = parsed.diffuse() {
                    stem = Some(d.stem().to_string());
                }
                if let Some(mode) = parsed.blend_mode {
                    blend[index] = Some((mode, parsed.mask_reference.unwrap_or(128)));
                }
            } else {
                eprintln!("{path}: material file {file} not found in _compiledMaterial.pak");
            }
            let stem = stem.or_else(|| material.diffuse_texture().map(str::to_string));
            eprintln!(
                "  material {index} {:?}: file {file}, diffuse {stem:?}",
                material.name
            );
            if let Some(stem) = stem {
                let key = format!("{}.ximg", stem.to_lowercase());
                let image = *image_cache.entry(key.clone()).or_insert_with(|| {
                    let at = *images_index.get(&key)?;
                    let bytes = image_pak.read_payload(at).ok()?;
                    let ximg = Ximg::parse(&bytes).ok()?;
                    images.push(ImageData {
                        width: ximg.width,
                        height: ximg.height,
                        fourcc: ximg.fourcc,
                        mips: ximg.mips().iter().map(|m| m.to_vec()).collect(),
                    });
                    Some(images.len() - 1)
                });
                if image.is_none() {
                    eprintln!("{path}: texture {stem} not found as ximg");
                }
                textures[index] = image;
            }
        }
        eprintln!(
            "actor {path}: {} nodes, {} meshes, {} materials ({} textured)",
            actor.nodes.len(),
            actor.meshes.len(),
            actor.materials.len() - 1,
            textures.iter().flatten().count()
        );
        actors.push(ActorData {
            name: path,
            actor,
            textures,
            blend,
        });
    }

    let motion = {
        let wanted = motion_arg.unwrap_or("idle");
        let resolved = match wanted.to_lowercase().as_str() {
            "idle" => "Hero_Stand_None_None_P0_Ambient_Loop_N_Fwd_01_%_00_P0_0",
            "walk" => "Hero_Stand_None_None_P0_Move_Walk_N_Fwd_00_%_00_P0_160",
            "run" => "Hero_Stand_None_None_P0_Move_Run_N_Fwd_00_%_00_P0_400",
            "none" => "",
            _ => wanted,
        };
        if resolved.is_empty() {
            None
        } else {
            let at = find_member(&animation, resolved, ".xmot")?;
            let path = animation.entries[at].path.clone();
            let motion = Motion::parse(&animation.read_payload(at)?)
                .map_err(|e| e.context(format!("parsing {path}")))?;
            eprintln!(
                "motion {path}: {} nodes, {} keys, {:.2}s",
                motion.nodes.len(),
                motion.key_count,
                motion.duration
            );
            Some((path, motion))
        }
    };
    ensure!(!actors.is_empty(), "no actors loaded");
    let label = format!(
        "{} | {}",
        actors
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>()
            .join(" + "),
        motion.as_ref().map_or("bind pose", |(n, _)| n.as_str())
    );
    Ok(Scene {
        actors,
        images,
        motion,
        label,
        yaw_degrees,
        in_place: !root_motion,
    })
}

fn texture_image(image: &ImageData) -> Option<Image> {
    let format = match &image.fourcc {
        b"DXT1" => TextureFormat::Bc1RgbaUnormSrgb,
        b"DXT3" => TextureFormat::Bc2RgbaUnormSrgb,
        b"DXT5" => TextureFormat::Bc3RgbaUnormSrgb,
        _ => return None,
    };
    if !image.width.is_multiple_of(4) || !image.height.is_multiple_of(4) {
        return None;
    }
    let data: Vec<u8> = image.mips.iter().flatten().copied().collect();
    let mut out = Image::new(
        Extent3d {
            width: image.width,
            height: image.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    out.texture_descriptor.mip_level_count = image.mips.len() as u32;
    out.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..default()
    });
    Some(out)
}

fn node_local(n: &gothic_formats::xact::ActorNode) -> (Vec3, Quat, Vec3) {
    (
        Vec3::from(n.position),
        quat(n.rotation),
        Vec3::from(n.scale),
    )
}

fn quat(q: [f32; 4]) -> Quat {
    let q = Quat::from_xyzw(q[0], q[1], q[2], q[3]);
    if q.length_squared() > 1e-12 {
        q.normalize()
    } else {
        Quat::IDENTITY
    }
}

/// Bind-pose model-space matrix of every node (parents resolved by name).
fn bind_world(actor: &Actor) -> Vec<Mat4> {
    let mut world: Vec<Option<Mat4>> = vec![None; actor.nodes.len()];
    fn resolve(actor: &Actor, world: &mut [Option<Mat4>], i: usize) -> Mat4 {
        if let Some(m) = world[i] {
            return m;
        }
        let (t, r, s) = node_local(&actor.nodes[i]);
        let local = Mat4::from_scale_rotation_translation(s, r, t);
        let m = match actor.nodes[i].parent {
            Some(p) => resolve(actor, world, p) * local,
            None => local,
        };
        world[i] = Some(m);
        m
    }
    (0..actor.nodes.len())
        .map(|i| resolve(actor, &mut world, i))
        .collect()
}

fn alpha_mode(blend: Option<(u32, u8)>) -> AlphaMode {
    match blend {
        Some((1, mask)) => AlphaMode::Mask(f32::from(mask) / 255.0),
        Some((2 | 8, _)) => AlphaMode::Blend,
        Some((7, _)) => AlphaMode::Add,
        _ => AlphaMode::Opaque,
    }
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
    scene: Res<Scene>,
) {
    let handles: Vec<Option<Handle<Image>>> = scene
        .images
        .iter()
        .map(|i| texture_image(i).map(|t| images.add(t)))
        .collect();
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    let mut map = Vec::new();
    let mut bind = Vec::new();
    let mut prefix = Vec::new();
    let mut root_flags: Vec<Vec<bool>> = Vec::new();
    for (actor_index, data) in scene.actors.iter().enumerate() {
        let actor = &data.actor;
        let world = bind_world(actor);
        let root_entity = commands
            .spawn((Transform::default(), Visibility::default()))
            .id();
        // One entity per node, in node order so joint indices equal node indices.
        let mut joints = Vec::new();
        for (node_index, node) in actor.nodes.iter().enumerate() {
            let (t, r, s) = node_local(node);
            joints.push(
                commands
                    .spawn((
                        Joint {
                            actor: actor_index,
                            node: node_index,
                        },
                        Transform {
                            translation: t,
                            rotation: r,
                            scale: s,
                        },
                    ))
                    .id(),
            );
        }
        for (node_index, node) in actor.nodes.iter().enumerate() {
            let parent = node.parent.map_or(root_entity, |p| joints[p]);
            commands.entity(parent).add_child(joints[node_index]);
        }
        let inverse = bindposes.add(world.iter().map(Mat4::inverse).collect::<Vec<_>>());
        // Name -> motion node mapping (first occurrence).
        if let Some((_, motion)) = &scene.motion {
            let by_name: HashMap<&str, usize> = motion
                .nodes
                .iter()
                .enumerate()
                .rev()
                .map(|(i, n)| (n.name.as_str(), i))
                .collect();
            let mapping: Vec<Option<usize>> = actor
                .nodes
                .iter()
                .map(|n| by_name.get(n.name.as_str()).copied())
                .collect();
            eprintln!(
                "{}: {}/{} nodes animated by the motion",
                data.name,
                mapping.iter().flatten().count(),
                actor.nodes.len()
            );
            let mut pre = vec![Mat4::IDENTITY; actor.nodes.len()];
            let mut anim_root = vec![false; actor.nodes.len()];
            for n in 0..actor.nodes.len() {
                if mapping[n].is_none() {
                    continue;
                }
                let parent = actor.nodes[n].parent;
                let mut anim_parent = parent;
                while let Some(p) = anim_parent {
                    if mapping[p].is_some() {
                        break;
                    }
                    anim_parent = actor.nodes[p].parent;
                }
                pre[n] = match (parent, anim_parent) {
                    (Some(p), Some(a)) => {
                        // inverse of the bind transform from the animated ancestor to the parent
                        (world[a].inverse() * world[p]).inverse()
                    }
                    (Some(p), None) => world[p].inverse(),
                    (None, _) => Mat4::IDENTITY,
                };
                anim_root[n] = anim_parent.is_none();
            }
            prefix.push(pre);
            root_flags.push(anim_root);
            map.push(mapping);
        } else {
            prefix.push(vec![Mat4::IDENTITY; actor.nodes.len()]);
            root_flags.push(vec![false; actor.nodes.len()]);
            map.push(vec![None; actor.nodes.len()]);
        }
        bind.push(actor.nodes.iter().map(node_local).collect());

        let mut fallback_material: Option<Handle<StandardMaterial>> = None;
        let mut material_cache: HashMap<u8, Handle<StandardMaterial>> = HashMap::new();
        for mesh_data in &actor.meshes {
            for sub in &mesh_data.submeshes {
                for p in &sub.positions {
                    let v = Vec3::from(*p);
                    min = min.min(v);
                    max = max.max(v);
                }
                let mut indices = sub.indices.clone();
                // Same winding convention as the static mesh viewer.
                for triangle in indices.as_chunks_mut::<3>().0 {
                    triangle.swap(1, 2);
                }
                let mut mesh = Mesh::new(
                    PrimitiveTopology::TriangleList,
                    RenderAssetUsages::RENDER_WORLD,
                )
                .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, sub.positions.clone())
                .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, sub.normals.clone())
                .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, sub.uvs.clone())
                .with_inserted_attribute(
                    Mesh::ATTRIBUTE_JOINT_INDEX,
                    VertexAttributeValues::Uint16x4(sub.joints.clone()),
                )
                .with_inserted_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, sub.weights.clone())
                .with_inserted_indices(Indices::U32(indices));
                mesh.asset_usage = RenderAssetUsages::RENDER_WORLD;
                let material = if sub.material == 0 {
                    fallback_material
                        .get_or_insert_with(|| {
                            materials.add(StandardMaterial {
                                base_color: Color::srgb(0.6, 0.6, 0.65),
                                cull_mode: None,
                                ..default()
                            })
                        })
                        .clone()
                } else {
                    material_cache
                        .entry(sub.material)
                        .or_insert_with(|| {
                            let texture = data
                                .textures
                                .get(sub.material as usize)
                                .copied()
                                .flatten()
                                .and_then(|i| handles[i].clone());
                            materials.add(StandardMaterial {
                                base_color: if texture.is_some() {
                                    Color::WHITE
                                } else {
                                    Color::srgb(0.6, 0.55, 0.5)
                                },
                                base_color_texture: texture,
                                perceptual_roughness: 0.85,
                                reflectance: 0.1,
                                alpha_mode: alpha_mode(
                                    data.blend.get(sub.material as usize).copied().flatten(),
                                ),
                                cull_mode: None,
                                ..default()
                            })
                        })
                        .clone()
                };
                let mesh_entity = commands
                    .spawn((
                        Mesh3d(meshes.add(mesh)),
                        MeshMaterial3d(material),
                        SkinnedMesh {
                            inverse_bindposes: inverse.clone(),
                            joints: joints.clone(),
                        },
                        // Joint-driven bounds are not generated; the actor is always framed.
                        NoFrustumCulling,
                    ))
                    .id();
                commands.entity(root_entity).add_child(mesh_entity);
            }
        }
    }
    commands.insert_resource(Playback {
        time: 0.0,
        paused: false,
        map,
        bind,
        root: root_flags,
        prefix,
    });
    let center = (min + max) * 0.5;
    let extent = (max - min).max_element().max(1.0);
    let distance = extent * 1.5;
    commands.spawn((
        Camera3d::default(),
        Orbit {
            target: center,
            distance,
            yaw: scene.yaw_degrees.to_radians(),
            pitch: 0.08,
        },
        Transform::default(),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 20000.0,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.7, 0.6, 0.0)),
    ));
    commands.insert_resource(GlobalAmbientLight {
        brightness: 900.0,
        ..default()
    });
    eprintln!("bounds min {min:?} max {max:?}");
}

fn sample<T: Copy>(keys: &[(f32, T)], t: f32, lerp: impl Fn(T, T, f32) -> T) -> Option<T> {
    let first = keys.first()?;
    let last = keys.last()?;
    if t <= first.0 {
        return Some(first.1);
    }
    if t >= last.0 {
        return Some(last.1);
    }
    let after = keys.partition_point(|k| k.0 <= t);
    let (a, b) = (keys[after - 1], keys[after]);
    let span = (b.0 - a.0).max(1e-6);
    Some(lerp(a.1, b.1, ((t - a.0) / span).clamp(0.0, 1.0)))
}

fn animate(
    time: Res<Time>,
    scene: Res<Scene>,
    smoke: Res<Smoke>,
    mut playback: ResMut<Playback>,
    mut joints: Query<(&Joint, &mut Transform)>,
) {
    let Some((_, motion)) = &scene.motion else {
        return;
    };
    if smoke.enabled {
        // Deterministic sampling for screenshots: times are driven by the smoke schedule.
        playback.time = match smoke.frame {
            0..=99 => smoke.times[0],
            _ => smoke.times[1],
        };
    } else if !playback.paused {
        playback.time += time.delta_secs();
    }
    let t = if motion.duration > 1e-3 && !smoke.enabled {
        playback.time % motion.duration
    } else {
        playback.time
    };
    for (joint, mut transform) in &mut joints {
        let (bind_t, bind_r, bind_s) = playback.bind[joint.actor][joint.node];
        let Some(m) = playback.map[joint.actor][joint.node].map(|i| &motion.nodes[i]) else {
            *transform = Transform {
                translation: bind_t,
                rotation: bind_r,
                scale: bind_s,
            };
            continue;
        };
        let pos = sample(&m.positions.keys, t, |a, b, f| {
            Vec3::from(a).lerp(Vec3::from(b), f).to_array()
        })
        .map(Vec3::from)
        // No position track: the node record's static pose.
        .unwrap_or(Vec3::from(m.position));
        let rot = sample(&m.rotations.keys, t, |a, b, f| {
            let (a, mut b) = (quat(a), quat(b));
            if a.dot(b) < 0.0 {
                b = -b;
            }
            a.slerp(b, f).to_array()
        })
        .map(quat)
        .unwrap_or_else(|| quat(m.rotation));
        let scale = sample(&m.scales.keys, t, |a, b, f| {
            Vec3::from(a).lerp(Vec3::from(b), f).to_array()
        })
        .map(Vec3::from)
        .unwrap_or(Vec3::from(m.scale));
        let mut pos = pos;
        if scene.in_place && playback.root[joint.actor][joint.node] {
            // Root motion: keep the root at its bind position.
            pos = Vec3::from(m.position);
        }
        let local = Mat4::from_scale_rotation_translation(scale, rot, pos);
        *transform = Transform::from_matrix(playback.prefix[joint.actor][joint.node] * local);
    }
}

fn orbit(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mouse: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    time: Res<Time>,
    mut playback: ResMut<Playback>,
    mut camera: Query<(&mut Orbit, &mut Transform)>,
) {
    let Ok((mut orbit, mut transform)) = camera.single_mut() else {
        return;
    };
    if buttons.pressed(MouseButton::Left) || buttons.pressed(MouseButton::Right) {
        orbit.yaw -= mouse.delta.x * 0.005;
        orbit.pitch = (orbit.pitch + mouse.delta.y * 0.005).clamp(-1.4, 1.4);
    }
    let dt = time.delta_secs().min(0.1);
    if keys.pressed(KeyCode::KeyA) {
        orbit.yaw += dt;
    }
    if keys.pressed(KeyCode::KeyD) {
        orbit.yaw -= dt;
    }
    if keys.pressed(KeyCode::KeyW) {
        orbit.distance *= 1.0 - dt;
    }
    if keys.pressed(KeyCode::KeyS) {
        orbit.distance *= 1.0 + dt;
    }
    orbit.distance = (orbit.distance * (1.0 - scroll.delta.y * 0.1)).max(5.0);
    if keys.just_pressed(KeyCode::Space) {
        playback.paused = !playback.paused;
    }
    let direction = Quat::from_euler(EulerRot::YXZ, orbit.yaw, -orbit.pitch, 0.0) * Vec3::Z;
    transform.translation = orbit.target + direction * orbit.distance;
    transform.look_at(orbit.target, Vec3::Y);
}

fn smoke_capture(mut commands: Commands, mut smoke: ResMut<Smoke>) {
    if !smoke.enabled {
        return;
    }
    smoke.frame += 1;
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
    if smoke.frame == 80 {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(smoke.path.clone()));
    }
    if smoke.frame == 180 {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(smoke.second_path.clone()));
    }
    if smoke.frame == 260 {
        commands.write_message(bevy::app::AppExit::Success);
    }
}

fn update_title(scene: Res<Scene>, playback: Res<Playback>, mut windows: Query<&mut Window>) {
    for mut window in &mut windows {
        window.title = format!(
            "Gothic Reforged | {} | t={:.2}s | LMB drag / A D / W S / Space",
            scene.label, playback.time
        );
    }
}
