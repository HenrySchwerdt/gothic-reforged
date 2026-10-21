//! A [`BuildSource`] that makes characters procedurally: a joint tree, skinned cylinder
//! meshes per part and generated textures. It costs about what a real character costs to
//! build (tens of thousands of vertices, 512² textures per part), so tests and the benchmark
//! exercise the streamer without game data.
use crate::pose::Trs;
use crate::rig::{NONE, Rig};
use crate::stream::{
    ActorBundle, BuildRequest, BuildSource, CpuAttachment, CpuMaterial, CpuMesh, CpuPart,
};
use anyhow::{Result, bail};
use bevy::asset::RenderAssetUsages;
use bevy::camera::primitives::Aabb;
use bevy::math::{Affine3A, Mat4, Quat, Vec3, Vec3A};
use bevy::mesh::{Indices, Mesh, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::{Color, Image, StandardMaterial};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct SyntheticSource {
    /// Skeleton joints (a binary tree; two of them are named `Slot_RightHand` / `Slot_Back`).
    pub joints: usize,
    /// Vertices per part mesh (about).
    pub vertices_per_part: usize,
    /// Texture edge per part (RGBA8, with mips).
    pub texture_size: u32,
    /// Parts whose name contains this fail to build (to test failure handling).
    pub fail_marker: Option<String>,
}

impl Default for SyntheticSource {
    fn default() -> Self {
        Self {
            joints: 64,
            vertices_per_part: 6000,
            texture_size: 512,
            fail_marker: None,
        }
    }
}

fn hash(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    x ^ (x >> 33)
}

fn seed_of(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

impl SyntheticSource {
    fn rig(&self) -> Rig {
        let n = self.joints.max(4);
        let mut names: Vec<String> = (0..n).map(|i| format!("Syn_Bone_{i}")).collect();
        names[n - 1] = "Slot_RightHand".into();
        names[n - 2] = "Slot_Back".into();
        let parent: Vec<u16> = (0..n)
            .map(|i| if i == 0 { NONE } else { ((i - 1) / 2) as u16 })
            .collect();
        let bind_local = (0..n)
            .map(|i| Trs {
                t: if i == 0 {
                    Vec3::new(0.0, 90.0, 0.0)
                } else {
                    Vec3::new(if i % 2 == 0 { 6.0 } else { -6.0 }, 12.0, 0.0)
                },
                r: Quat::from_rotation_y(0.05 * i as f32),
                s: Vec3::ONE,
            })
            .collect();
        Rig::from_parts("synthetic", names, parent, bind_local)
    }

    /// Skinned cylinder around the skeleton's height, rings bound to joints in turn.
    fn mesh(&self, seed: u64, joints: usize) -> (Mesh, Aabb) {
        let segments = 24usize;
        let rings = (self.vertices_per_part / segments).max(2);
        let mut positions = Vec::with_capacity(rings * segments);
        let mut normals = Vec::with_capacity(rings * segments);
        let mut uvs = Vec::with_capacity(rings * segments);
        let mut joint_idx = Vec::with_capacity(rings * segments);
        let mut weights = Vec::with_capacity(rings * segments);
        let radius = 15.0 + (seed % 10) as f32;
        for r in 0..rings {
            let v = r as f32 / (rings - 1) as f32;
            let y = v * 180.0;
            let j0 = ((v * (joints - 1) as f32) as usize).min(joints - 1);
            let j1 = (j0 + 1).min(joints - 1);
            let w1 = (v * (joints - 1) as f32).fract();
            for s in 0..segments {
                let a = s as f32 / segments as f32 * std::f32::consts::TAU;
                let (sin, cos) = a.sin_cos();
                let wobble = 1.0 + 0.1 * ((hash(seed ^ (r * 31 + s) as u64) % 100) as f32 / 100.0);
                positions.push([cos * radius * wobble, y, sin * radius * wobble]);
                normals.push([cos, 0.0, sin]);
                uvs.push([s as f32 / segments as f32, v]);
                joint_idx.push([j0 as u16, j1 as u16, 0, 0]);
                weights.push([1.0 - w1, w1, 0.0, 0.0]);
            }
        }
        let mut indices = Vec::with_capacity((rings - 1) * segments * 6);
        for r in 0..rings - 1 {
            for s in 0..segments {
                let a = (r * segments + s) as u32;
                let b = (r * segments + (s + 1) % segments) as u32;
                let c = a + segments as u32;
                let d = b + segments as u32;
                indices.extend([a, c, b, b, c, d]);
            }
        }
        let aabb = Aabb {
            center: Vec3A::new(0.0, 90.0, 0.0),
            half_extents: Vec3A::new(radius * 1.2, 95.0, radius * 1.2),
        };
        let mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_JOINT_INDEX,
            VertexAttributeValues::Uint16x4(joint_idx),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, weights)
        .with_inserted_indices(Indices::U32(indices));
        (mesh, aabb)
    }

    /// Procedural RGBA texture with a full mip chain (the decode / mip work a real texture
    /// costs).
    fn texture(&self, seed: u64) -> Image {
        let size = self.texture_size.max(4);
        let mut data = Vec::new();
        let mut level = Vec::with_capacity((size * size * 4) as usize);
        for y in 0..size {
            for x in 0..size {
                let h = hash(seed ^ (u64::from(y) << 20) ^ u64::from(x));
                let v = ((x / 16 + y / 16) % 2) as u8 * 60;
                level.extend([v + (h & 63) as u8, v + ((h >> 8) & 63) as u8, 120, 255]);
            }
        }
        let mut edge = size;
        let mut mips = 0;
        loop {
            data.extend_from_slice(&level);
            mips += 1;
            if edge == 1 {
                break;
            }
            let half = (edge / 2).max(1);
            let mut next = Vec::with_capacity((half * half * 4) as usize);
            for y in 0..half {
                for x in 0..half {
                    for c in 0..4 {
                        let at = |dx: u32, dy: u32| {
                            u32::from(level[(((y * 2 + dy) * edge + x * 2 + dx) * 4 + c) as usize])
                        };
                        next.push(((at(0, 0) + at(1, 0) + at(0, 1) + at(1, 1)) / 4) as u8);
                    }
                }
            }
            level = next;
            edge = half;
        }
        // `Image::new` only accepts level 0; the mip chain follows it in `data`.
        let mut image = Image::new_uninit(
            Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        );
        image.data = Some(data);
        image.texture_descriptor.mip_level_count = mips;
        image
    }

    fn cpu_mesh(&self, name: &str, joints: usize) -> CpuMesh {
        let seed = seed_of(name);
        let (mesh, _) = self.mesh(seed, joints);
        CpuMesh {
            mesh,
            material: CpuMaterial {
                material: StandardMaterial {
                    base_color: Color::WHITE,
                    perceptual_roughness: 0.8,
                    ..Default::default()
                },
                texture: Some((format!("syn_tex_{name}"), Arc::new(self.texture(seed)))),
            },
        }
    }
}

impl BuildSource for SyntheticSource {
    fn build(&self, request: &BuildRequest) -> Result<ActorBundle> {
        if request.parts.is_empty() {
            bail!("a character needs a body part");
        }
        if let Some(marker) = &self.fail_marker
            && request.parts.iter().any(|p| p.contains(marker.as_str()))
        {
            bail!("synthetic failure for {:?}", request.parts);
        }
        let rig = Arc::new(self.rig());
        let n = rig.names.len();
        let joint_nodes: Arc<[u16]> = (0..n as u16).collect::<Vec<_>>().into();
        let plan = rig.plan(&joint_nodes).into();
        let joint_by_name: HashMap<String, u16> = rig
            .names
            .iter()
            .enumerate()
            .map(|(i, name)| (name.to_lowercase(), i as u16))
            .collect();
        let inverse_bindposes: Vec<Mat4> = rig.bind_world.iter().map(|m| m.inverse()).collect();
        let parts = request
            .parts
            .iter()
            .map(|name| {
                let mesh = self.cpu_mesh(name, n);
                let (_, aabb) = self.mesh(seed_of(name), n);
                CpuPart {
                    name: name.clone(),
                    joints: (0..n as u16).collect(),
                    joint_rig_nodes: (0..n as u16).collect(),
                    inverse_bindposes: inverse_bindposes.clone(),
                    meshes: vec![mesh],
                    aabb,
                }
            })
            .collect();
        let attachments = request
            .attachments
            .iter()
            .map(|a| {
                let slot = |s: &str| {
                    joint_by_name
                        .get(&s.to_lowercase())
                        .copied()
                        .ok_or_else(|| anyhow::anyhow!("rig has no slot {s:?}"))
                };
                let joint = slot(&a.slot)?;
                let weapon = match &a.holster_slot {
                    Some(h) => Some((joint, slot(h)?)),
                    None => None,
                };
                Ok(CpuAttachment {
                    meshes: vec![CpuMesh {
                        mesh: bevy::prelude::Cuboid::new(4.0, 80.0, 4.0).into(),
                        material: CpuMaterial {
                            material: StandardMaterial::default(),
                            texture: None,
                        },
                    }],
                    joint,
                    offset: Affine3A::IDENTITY,
                    weapon,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(ActorBundle {
            key: request.key(),
            rig,
            plan,
            joint_nodes,
            facing_fix: Quat::IDENTITY,
            joint_by_name,
            parts,
            attachments,
            motion: None,
            locomotion: false,
        })
    }
}
