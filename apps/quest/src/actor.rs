//! Cooked actors (`.gact`) as Bevy skinned meshes sharing one joint hierarchy (body + head parts bind by
//! node name, as in the original). Gothic data is left-handed: like the engine port (`crates/animation`,
//! commit "Fix global left/right mirroring of all actors") skeletons and meshes are mirrored across X on load
//! (positions and normals x -> -x, rotations (x,y,z,w) -> (x,-y,-z,w)); the reflection reverses the winding, so no
//! index swap is needed. Units are centimetres; the caller scales the root by 1/100.
use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::{NoFrustumCulling, RenderLayers};
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use gothic_cook::AssetId;
use gothic_cook::actor::ActorNodeRec;

use crate::ardea::Ardea;
use crate::qmat::{QParams, QuestMaterial};

/// Shared skeleton of a spawned character (joint entity per body-actor node, mirrored bind pose).
#[derive(Component)]
pub struct ActorRig {
    pub root: Entity,
    pub by_name: HashMap<String, usize>,
    pub names: Vec<String>,
    pub parent: Vec<i32>,
    /// Bind-pose local transforms (cm, mirrored).
    pub bind_local: Vec<Transform>,
    /// Bind-pose model-space matrices.
    pub bind_global: Vec<Mat4>,
    pub joints: Vec<Entity>,
}

impl ActorRig {
    pub fn empty() -> Self {
        Self {
            root: Entity::PLACEHOLDER,
            by_name: Default::default(),
            names: vec![],
            parent: vec![],
            bind_local: vec![],
            bind_global: vec![],
            joints: vec![],
        }
    }
}

fn mirror_quat(q: [f32; 4]) -> Quat {
    Quat::from_xyzw(q[0], -q[1], -q[2], q[3]).normalize()
}

fn node_local(n: &ActorNodeRec) -> Transform {
    Transform {
        translation: Vec3::new(-n.position[0], n.position[1], n.position[2]),
        rotation: mirror_quat(n.rotation),
        scale: Vec3::from(n.scale),
    }
}

/// Where a part's meshes go.
#[derive(Clone, Copy)]
pub struct PartOptions {
    /// Alpha-tested materials (hair, beard, eyelashes).
    pub cutout: bool,
    /// Hidden from the player's own eyes (render layer 1 only).
    pub hide_from_eyes: bool,
}

/// Spawns `actor_name` as the skeleton owner under `root`.
pub fn spawn_body(
    commands: &mut Commands,
    a: &mut Ardea,
    actor_name: &str,
    root: Entity,
    opts: PartOptions,
    meshes: &mut Assets<Mesh>,
    qmats: &mut Assets<QuestMaterial>,
    images: &mut Assets<Image>,
    inv: &mut Assets<SkinnedMeshInverseBindposes>,
) -> Option<ActorRig> {
    let data = a.cache().load_actor(AssetId::of(actor_name)).ok()?;
    let names = data.node_names();
    let nodes = data.nodes();
    let n = nodes.len();
    let mut joints = Vec::with_capacity(n);
    let mut bind_local = Vec::with_capacity(n);
    let mut parent = Vec::with_capacity(n);
    for rec in nodes {
        let t = node_local(rec);
        bind_local.push(t);
        parent.push(rec.parent);
        joints.push(commands.spawn((t, Visibility::default())).id());
    }
    let mut bind_global = vec![Mat4::IDENTITY; n];
    // Parents are not guaranteed to precede children: resolve iteratively.
    let mut done = vec![false; n];
    let mut left = n;
    while left > 0 {
        let before = left;
        for i in 0..n {
            if done[i] {
                continue;
            }
            let local = bind_local[i].to_matrix();
            if parent[i] < 0 {
                bind_global[i] = local;
            } else if done[parent[i] as usize] {
                bind_global[i] = bind_global[parent[i] as usize] * local;
            } else {
                continue;
            }
            done[i] = true;
            left -= 1;
        }
        if left == before {
            break;
        }
    }
    for i in 0..n {
        let p = if parent[i] >= 0 {
            joints[parent[i] as usize]
        } else {
            root
        };
        commands.entity(joints[i]).insert(ChildOf(p));
    }
    let by_name = names
        .iter()
        .enumerate()
        .map(|(i, nm)| (nm.clone(), i))
        .collect();
    let rig = ActorRig {
        root,
        by_name,
        names,
        parent,
        bind_local,
        bind_global,
        joints,
    };
    add_part(commands, a, &data, &rig, opts, meshes, qmats, images, inv);
    Some(rig)
}

/// Adds another actor (head, armor) bound to the rig by node name.
pub fn spawn_part(
    commands: &mut Commands,
    a: &mut Ardea,
    actor_name: &str,
    rig: &ActorRig,
    opts: PartOptions,
    meshes: &mut Assets<Mesh>,
    qmats: &mut Assets<QuestMaterial>,
    images: &mut Assets<Image>,
    inv: &mut Assets<SkinnedMeshInverseBindposes>,
) -> Option<()> {
    let data = a.cache().load_actor(AssetId::of(actor_name)).ok()?;
    add_part(commands, a, &data, rig, opts, meshes, qmats, images, inv);
    Some(())
}

fn add_part(
    commands: &mut Commands,
    a: &mut Ardea,
    data: &gothic_cook::actor::ActorData,
    rig: &ActorRig,
    opts: PartOptions,
    meshes: &mut Assets<Mesh>,
    qmats: &mut Assets<QuestMaterial>,
    images: &mut Assets<Image>,
    inv: &mut Assets<SkinnedMeshInverseBindposes>,
) {
    let names = data.node_names();
    let nodes = data.nodes();
    let mut part_global = vec![Mat4::IDENTITY; nodes.len()];
    let mut done = vec![false; nodes.len()];
    let mut left = nodes.len();
    while left > 0 {
        let before = left;
        for (i, rec) in nodes.iter().enumerate() {
            if done[i] {
                continue;
            }
            let l = node_local(rec).to_matrix();
            if rec.parent < 0 {
                part_global[i] = l;
            } else if done[rec.parent as usize] {
                part_global[i] = part_global[rec.parent as usize] * l;
            } else {
                continue;
            }
            done[i] = true;
            left -= 1;
        }
        if left == before {
            break;
        }
    }
    let joint_entities: Vec<Entity> = names
        .iter()
        .map(|nm| rig.by_name.get(nm).map_or(rig.root, |&i| rig.joints[i]))
        .collect();
    let ibp = inv.add(SkinnedMeshInverseBindposes::from(
        part_global.iter().map(|m| m.inverse()).collect::<Vec<_>>(),
    ));
    let amats = data.materials();
    let verts = data.vertices();
    let idx = data.indices();
    let layers = if opts.hide_from_eyes {
        RenderLayers::layer(1)
    } else {
        RenderLayers::layer(0).with(1)
    };
    for sub in data.submeshes() {
        let vs = &verts[sub.first_vertex as usize..(sub.first_vertex + sub.vertex_count) as usize];
        let is = &idx[sub.first_index as usize..(sub.first_index + sub.index_count) as usize];
        let mut m = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        m.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vs.iter()
                .map(|v| [-v.pos[0], v.pos[1], v.pos[2]])
                .collect::<Vec<_>>(),
        );
        m.insert_attribute(
            Mesh::ATTRIBUTE_NORMAL,
            vs.iter()
                .map(|v| [-v.normal[0], v.normal[1], v.normal[2]])
                .collect::<Vec<_>>(),
        );
        m.insert_attribute(
            Mesh::ATTRIBUTE_UV_0,
            vs.iter().map(|v| v.uv).collect::<Vec<_>>(),
        );
        m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1.0f32; 4]; vs.len()]);
        if sub.skinned != 0 {
            m.insert_attribute(
                Mesh::ATTRIBUTE_JOINT_INDEX,
                VertexAttributeValues::Uint16x4(vs.iter().map(|v| v.joints).collect()),
            );
            m.insert_attribute(
                Mesh::ATTRIBUTE_JOINT_WEIGHT,
                vs.iter()
                    .map(|v| {
                        [
                            v.weights[0] as f32 / 255.0,
                            v.weights[1] as f32 / 255.0,
                            v.weights[2] as f32 / 255.0,
                            v.weights[3] as f32 / 255.0,
                        ]
                    })
                    .collect::<Vec<[f32; 4]>>(),
            );
        }
        m.insert_indices(Indices::U32(is.to_vec()));
        let am = amats.get(sub.material as usize);
        let mut from_shader = false;
        let tex = am
            .and_then(|am| a.texture(am.diffuse_tex, images))
            .or_else(|| {
                let t = am.and_then(|am| a.shader_diffuse(am.shader_material, images));
                from_shader = t.is_some();
                t
            });
        let (th, ts) = match tex {
            Some((h, s)) => (Some(h), s),
            None => (Some(a.white_image()), Vec2::ONE),
        };
        let tint = if th.is_some() && (from_shader || am.is_some_and(|am| am.diffuse_tex != 0)) {
            Vec3::ONE
        } else {
            am.map_or(Vec3::splat(0.6), |am| {
                Vec3::new(am.diffuse[0], am.diffuse[1], am.diffuse[2])
            })
        };
        let mat = qmats.add(QuestMaterial {
            params: QParams::new(tint, ts, if opts.cutout { 0.4 } else { 0.0 }),
            tex: th,
            tex_b: None,
            detail_a: None,
            detail_b: None,
            two_sided: true,
        });
        let node = sub.node as usize;
        let mut e = commands.spawn((
            Mesh3d(meshes.add(m)),
            MeshMaterial3d(mat),
            Transform::IDENTITY,
            NoFrustumCulling,
            layers.clone(),
        ));
        if sub.skinned != 0 {
            e.insert((
                SkinnedMesh {
                    inverse_bindposes: ibp.clone(),
                    joints: joint_entities.clone(),
                },
                ChildOf(rig.root),
            ));
        } else if node < joint_entities.len() {
            // Rigid part: attached to its node, vertices are in model space.
            e.insert((
                Transform::from_matrix(part_global[node].inverse()),
                ChildOf(joint_entities[node]),
            ));
        } else {
            e.insert(ChildOf(rig.root));
        }
    }
}
