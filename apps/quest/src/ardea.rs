//! Ardea from the cooked `quest`-profile cache subset (see examples/pack_ardea.rs): terrain + static props with
//! distance-based LOD, streamed cell by cell with a per-frame time budget. All data is the user's own local
//! cache pushed to the app's private external dir; nothing is bundled in the APK.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use bevy::asset::RenderAssetUsages;
use bevy::camera::primitives::Aabb;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AstcBlock, AstcChannel, Extent3d, TextureDimension, TextureFormat,
};
use gothic_cook::material::{FLAG_NO_LIGHTING, KIND_WATER, NONE_U32};
use gothic_cook::mesh::{Indices as CIdx, MeshData};
use gothic_cook::texture::{TEX_SRGB, TexFormat, TextureData};
use gothic_cook::{AssetId, Cache};
use gothic_xr_quest::knob_f32;

use crate::locomotion::{CM, Physics};
use crate::qmat::{QParams, QuestMaterial};
use gothic_vr::{VrPlayer, VrWorld};

/// Sample colour textures raw (gamma space) for the original-shading shader.
pub const GAMMA_SPACE: bool = true;

/// Gothic-space start (Ardea plaza); Bevy x is mirrored.
pub const START_GOTHIC: [f32; 2] = [92_400.0, -11_000.0];

#[derive(Component)]
pub struct WorldRoot;
#[derive(Component)]
pub struct TriCount(pub u32);
#[derive(Component)]
pub struct StaticProp;

/// Per-entity LOD state: stage distances (cm) from `render-rules::lod::stage_distances`.
#[derive(Component)]
pub struct Lod {
    handles: [Option<Handle<Mesh>>; 3],
    tris: [u32; 3],
    stage_d: [f32; 2],
    levels: u8,
    cur: u8,
    center: Vec3,
    cull_end: f32,
    hidden: bool,
}

/// Switches LOD stages and culls small objects by distance to the eyes (original thresholds).
pub fn update_lod(
    eyes: Query<&GlobalTransform, With<gothic_xr_quest::XrEye>>,
    mut q: Query<(&mut Lod, &mut Mesh3d, &mut Visibility, &mut TriCount)>,
) {
    let Some(eye) = eyes.iter().next() else {
        return;
    };
    let cam = eye.translation() * CM;
    for (mut l, mut mesh, mut vis, mut tc) in &mut q {
        let d = (l.center - cam).length();
        let hide = d > l.cull_end;
        if hide != l.hidden {
            l.hidden = hide;
            *vis = if hide {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            };
        }
        if hide || l.levels <= 1 {
            continue;
        }
        let stage = if d > l.stage_d[0] {
            if l.levels > 2 && d > l.stage_d[1] {
                2
            } else {
                1
            }
        } else {
            0
        };
        // 5 % hysteresis against flicker at the boundary.
        let cur = l.cur;
        let edge = |i: usize| l.stage_d[i];
        let keep = match (cur, stage) {
            (0, 1) => d < edge(0) * 1.05,
            (1, 0) => d > edge(0) * 0.95,
            (1, 2) => d < edge(1) * 1.05,
            (2, 1) => d > edge(1) * 0.95,
            _ => false,
        };
        if stage != cur && !keep {
            if let Some(h) = l.handles[stage as usize].clone() {
                mesh.0 = h;
                tc.0 = l.tris[stage as usize];
                l.cur = stage;
            }
        }
    }
}

#[derive(Resource)]
pub struct Ardea {
    cache: Arc<Cache>,
    pending: Vec<(i32, i32)>,
    materials: HashMap<u64, Handle<StandardMaterial>>,
    qmaterials: HashMap<u64, Handle<QuestMaterial>>,
    cheap: bool,
    white: Handle<Image>,
    textures: HashMap<u64, Option<(Handle<Image>, Vec2)>>,
    meshes: HashMap<(u64, u32, u8), Handle<Mesh>>,
    root: Entity,
    focus: Vec3,
    pub cells_done: usize,
    pub spawned: usize,
    pub tris: u64,
    lod_scale: f32,
    cull_scale: f32,
    unlit: bool,
}

pub fn data_dir() -> Option<PathBuf> {
    let mut cands: Vec<PathBuf> = Vec::new();
    if let Some(d) = gothic_xr_quest::knob("data") {
        cands.push(d.into());
    }
    cands.push("/sdcard/Android/data/dev.gothicreforged.questtest/files/ardea".into());
    cands.push(".local/quest/ardea-pack".into());
    cands
        .into_iter()
        .find(|p| p.join("manifest.json").is_file())
}

pub fn build(app: &mut App, dir: PathBuf) -> anyhow::Result<()> {
    let cache = Arc::new(Cache::open(dir)?);
    let focus = Vec3::new(-START_GOTHIC[0], 0.0, START_GOTHIC[1]);
    let r = knob_f32("radius", 25_000.0);
    let c = START_GOTHIC;
    let mut pending = Vec::new();
    for cx in ((c[0] - r) / 10_000.0).floor() as i32..=((c[0] + r) / 10_000.0).floor() as i32 {
        for cz in ((c[1] - r) / 10_000.0).floor() as i32..=((c[1] + r) / 10_000.0).floor() as i32 {
            let (mx, mz) = (
                (cx as f32 + 0.5) * 10_000.0 - c[0],
                (cz as f32 + 0.5) * 10_000.0 - c[1],
            );
            if cache
                .root()
                .join(format!("cells/{cx}_{cz}.gcell"))
                .is_file()
                && (mx * mx + mz * mz).sqrt() < r + 7_100.0
            {
                pending.push((cx, cz, mx * mx + mz * mz));
            }
        }
    }
    pending.sort_by(|a, b| a.2.total_cmp(&b.2));
    let pending = pending.into_iter().map(|p| (p.0, p.1)).collect();
    let world = app.world_mut();
    let root = world
        .spawn((
            WorldRoot,
            Transform::from_scale(Vec3::splat(1.0 / CM)),
            Visibility::default(),
        ))
        .id();
    let white = world.resource_mut::<Assets<Image>>().add(Image::new_fill(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[255, 255, 255, 255],
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    ));
    world.insert_resource(Ardea {
        cache,
        pending,
        materials: HashMap::new(),
        qmaterials: HashMap::new(),
        cheap: gothic_xr_quest::knob("cheap").as_deref() != Some("0"),
        white,
        textures: HashMap::new(),
        meshes: HashMap::new(),
        root,
        focus,
        cells_done: 0,
        spawned: 0,
        tris: 0,
        lod_scale: knob_f32("lod_scale", 1.0),
        cull_scale: knob_f32("cull_scale", 1.0),
        unlit: gothic_xr_quest::knob("unlit_world").as_deref() == Some("1"),
    });
    let f = crate::qmat::fog_color();
    world.insert_resource(gothic_xr_quest::XrSettings {
        clear: Color::srgb(f.x, f.y, f.z),
        ..default()
    });
    let _ = world;
    crate::locomotion::init(app, Vec3::new(focus.x, 20_000.0, focus.z));
    app.add_systems(Update, (stream_cells, report, update_lod));
    Ok(())
}

fn build_image(t: &TextureData) -> Option<(Image, Vec2)> {
    let h = t.header();
    #[cfg(not(target_os = "android"))]
    if let Some(r) = host_astc(t) {
        return Some(r);
    }
    let srgb = h.flags & TEX_SRGB != 0;
    let format = match t.format() {
        TexFormat::Astc6x6 => TextureFormat::Astc {
            block: AstcBlock::B6x6,
            channel: if srgb && !GAMMA_SPACE {
                AstcChannel::UnormSrgb
            } else {
                AstcChannel::Unorm
            },
        },
        TexFormat::Rgba8 => {
            if srgb && !GAMMA_SPACE {
                TextureFormat::Rgba8UnormSrgb
            } else {
                TextureFormat::Rgba8Unorm
            }
        }
        _ => return None,
    };
    let mut img = Image::default();
    img.data = Some(t.data().to_vec());
    // wgpu requires block-multiple base dimensions; the cooked ASTC data is already whole blocks, so the texture
    // is created padded and the UVs are scaled back to the real content.
    let (pw, ph, uvs) = match t.format() {
        TexFormat::Astc6x6 => {
            let (pw, ph) = (h.width.div_ceil(6) * 6, h.height.div_ceil(6) * 6);
            (
                pw,
                ph,
                Vec2::new(h.width as f32 / pw as f32, h.height as f32 / ph as f32),
            )
        }
        _ => (h.width, h.height, Vec2::ONE),
    };
    img.texture_descriptor.size = Extent3d {
        width: pw,
        height: ph,
        depth_or_array_layers: 1,
    };
    img.texture_descriptor.dimension = TextureDimension::D2;
    img.texture_descriptor.format = format;
    img.texture_descriptor.mip_level_count = h.mip_count.max(1);
    img.asset_usage = RenderAssetUsages::RENDER_WORLD;
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 2,
        ..default()
    });
    Some((img, uvs))
}

/// Reflects a mesh across X (positions, normals, tangents) and reverses the triangle winding.
pub fn mirror_mesh_x(m: &mut Mesh) {
    use bevy::mesh::VertexAttributeValues as V;
    if let Some(V::Float32x3(p)) = m.attribute_mut(Mesh::ATTRIBUTE_POSITION) {
        for v in p.iter_mut() {
            v[0] = -v[0];
        }
    }
    if let Some(V::Float32x3(n)) = m.attribute_mut(Mesh::ATTRIBUTE_NORMAL) {
        for v in n.iter_mut() {
            v[0] = -v[0];
        }
    }
    match m.indices_mut() {
        Some(Indices::U16(i)) => i.chunks_exact_mut(3).for_each(|t| t.swap(1, 2)),
        Some(Indices::U32(i)) => i.chunks_exact_mut(3).for_each(|t| t.swap(1, 2)),
        None => {}
    }
}

fn build_mesh(md: &MeshData, s: &gothic_cook::mesh::SubmeshRec, lod: usize) -> (Mesh, u32) {
    let vs = md.submesh_vertices(s);
    let mut m = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    m.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vs.iter().map(|v| v.pos).collect::<Vec<_>>(),
    );
    m.insert_attribute(
        Mesh::ATTRIBUTE_NORMAL,
        vs.iter().map(|v| v.normal).collect::<Vec<_>>(),
    );
    m.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vs.iter().map(|v| v.uv).collect::<Vec<_>>(),
    );
    m.insert_attribute(
        Mesh::ATTRIBUTE_COLOR,
        vs.iter()
            .map(|v| {
                [
                    v.color[0] as f32 / 255.0,
                    v.color[1] as f32 / 255.0,
                    v.color[2] as f32 / 255.0,
                    v.color[3] as f32 / 255.0,
                ]
            })
            .collect::<Vec<[f32; 4]>>(),
    );
    let idx = md.indices(s, lod);
    let n = idx.len() as u32 / 3;
    m.insert_indices(match idx {
        CIdx::U16(i) => Indices::U16(i.to_vec()),
        CIdx::U32(i) => Indices::U32(i.to_vec()),
    });
    (m, n)
}

impl Ardea {
    pub fn cache(&self) -> &Cache {
        &self.cache
    }

    /// Spawns a cooked static mesh (all submeshes, LOD0) as children of `parent`.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_static(
        &mut self,
        commands: &mut Commands,
        mesh_name: &str,
        parent: Entity,
        local: Transform,
        meshes: &mut Assets<Mesh>,
        qmats: &mut Assets<QuestMaterial>,
        images: &mut Assets<Image>,
    ) -> bool {
        let Ok(md) = self.cache.load_mesh(AssetId::of(mesh_name)) else {
            return false;
        };
        let holder = commands
            .spawn((local, Visibility::default(), ChildOf(parent)))
            .id();
        for s in md.submeshes() {
            let Some(q) = self.qmaterial(s.material, qmats, images) else {
                continue;
            };
            let (mesh, _) = build_mesh(&md, s, 0);
            commands.spawn((
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(q),
                Transform::IDENTITY,
                Aabb::from_min_max(Vec3::from(s.bounds_min), Vec3::from(s.bounds_max)),
                ChildOf(holder),
            ));
        }
        true
    }

    /// Like [`Self::spawn_static`] for meshes that live in the left-handed Gothic frame but are attached to the mirrored
    /// skeletons/props: vertices and normals are reflected across X and the winding reversed (no negative scales, so
    /// back-face culling stays correct).
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_static_mirrored(
        &mut self,
        commands: &mut Commands,
        mesh_name: &str,
        parent: Entity,
        local: Transform,
        meshes: &mut Assets<Mesh>,
        qmats: &mut Assets<QuestMaterial>,
        images: &mut Assets<Image>,
    ) -> bool {
        let Ok(md) = self.cache.load_mesh(AssetId::of(mesh_name)) else {
            return false;
        };
        let holder = commands
            .spawn((local, Visibility::default(), ChildOf(parent)))
            .id();
        for s in md.submeshes() {
            let Some(q) = self.qmaterial(s.material, qmats, images) else {
                continue;
            };
            let (mut mesh, _) = build_mesh(&md, s, 0);
            mirror_mesh_x(&mut mesh);
            let (lo, hi) = (Vec3::from(s.bounds_min), Vec3::from(s.bounds_max));
            commands.spawn((
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(q),
                Transform::IDENTITY,
                Aabb::from_min_max(Vec3::new(-hi.x, lo.y, lo.z), Vec3::new(-lo.x, hi.y, hi.z)),
                ChildOf(holder),
            ));
        }
        true
    }

    /// Diffuse texture (and its UV scale) of a cooked shader material (`.xshmat`), for actor materials whose own
    /// layer carries no texture (heads: the skin texture lives in the shared material).
    pub fn shader_diffuse(
        &mut self,
        id: u64,
        images: &mut Assets<Image>,
    ) -> Option<(Handle<Image>, Vec2)> {
        if id == 0 {
            return None;
        }
        let rec = self.cache.material(id)?;
        let l = rec.layers[0];
        let (h, s) = self.texture(l.tex[0], images)?;
        Some((h, Vec2::new(l.scale[0][0], l.scale[0][1]) * s))
    }

    pub fn white_image(&self) -> Handle<Image> {
        self.white.clone()
    }

    pub fn texture(
        &mut self,
        id: u64,
        images: &mut Assets<Image>,
    ) -> Option<(Handle<Image>, Vec2)> {
        if id == 0 {
            return None;
        }
        if let Some(h) = self.textures.get(&id) {
            return h.clone();
        }
        let h = self
            .cache
            .load_texture(AssetId(id))
            .ok()
            .and_then(|t| build_image(&t))
            .map(|(i, uv)| (images.add(i), uv));
        self.textures.insert(id, h.clone());
        h
    }

    fn qmaterial(
        &mut self,
        id: u64,
        mats: &mut Assets<QuestMaterial>,
        images: &mut Assets<Image>,
    ) -> Option<Handle<QuestMaterial>> {
        if let Some(h) = self.qmaterials.get(&id) {
            return Some(h.clone());
        }
        let rec = self.cache.material(id)?;
        if rec.kind == KIND_WATER {
            return None;
        }
        let l = rec.layers[0];
        let tex = self.texture(l.tex[0], images);
        let texscale = tex.as_ref().map_or(Vec2::ONE, |t| t.1);
        let masked = rec.mask_reference != NONE_U32;
        let l1 = rec.layers[1];
        let blend =
            rec.flags & gothic_cook::material::FLAG_VERTEX_BLEND != 0 && rec.layer_count >= 2;
        let tex_b = if blend {
            self.texture(l1.tex[0], images)
        } else {
            None
        };
        let mut params = QParams::new(
            Vec3::new(
                l.tint[0].max(0.05),
                l.tint[1].max(0.05),
                l.tint[2].max(0.05),
            ),
            Vec2::new(l.scale[0][0], l.scale[0][1]) * texscale,
            if masked {
                (rec.mask_reference as f32 / 255.0).max(0.05)
            } else {
                0.0
            },
        );
        if let Some((_, sb)) = &tex_b {
            params.misc.y = 1.0;
            params.uv_scale_b = Vec4::new(l1.scale[0][0] * sb.x, l1.scale[0][1] * sb.y, 0.0, 0.0);
            params.tint_b = Vec3::new(
                l1.tint[0].max(0.05),
                l1.tint[1].max(0.05),
                l1.tint[2].max(0.05),
            )
            .extend(1.0);
        }
        let da = if l.tex[1] != 0 {
            self.texture(l.tex[1], images)
        } else {
            None
        };
        let db = if blend && l1.tex[1] != 0 {
            self.texture(l1.tex[1], images)
        } else {
            None
        };
        if let Some((_, sd)) = &da {
            params.flags.x = 1.0;
            params.uv_scale_d.x = l.scale[1][0] * sd.x;
            params.uv_scale_d.y = l.scale[1][1] * sd.y;
        }
        if let Some((_, sd)) = &db {
            params.flags.y = 1.0;
            params.uv_scale_d.z = l1.scale[1][0] * sd.x;
            params.uv_scale_d.w = l1.scale[1][1] * sd.y;
        }
        // Missing base texture: neutral grey instead of black.
        if tex.is_none() {
            params.tint = Vec4::new(0.5, 0.5, 0.5, 1.0);
        }
        let tex = tex.or_else(|| Some((self.white.clone(), Vec2::ONE)));
        let m = QuestMaterial {
            params,
            tex: tex.map(|t| t.0),
            tex_b: tex_b.map(|t| t.0),
            detail_a: da.map(|t| t.0),
            detail_b: db.map(|t| t.0),
            two_sided: masked,
        };
        let h = mats.add(m);
        self.qmaterials.insert(id, h.clone());
        Some(h)
    }

    fn material(
        &mut self,
        id: u64,
        mats: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
    ) -> Option<Handle<StandardMaterial>> {
        if let Some(h) = self.materials.get(&id) {
            return Some(h.clone());
        }
        let rec = self.cache.material(id)?;
        if rec.kind == KIND_WATER {
            return None;
        }
        let tex = self.texture(rec.layers[0].tex[0], images);
        let l = rec.layers[0];
        let texscale = tex.as_ref().map_or(Vec2::ONE, |t| t.1);
        let tex = tex.map(|t| t.0);
        let mut m = StandardMaterial {
            base_color: Color::srgb(
                l.tint[0].max(0.05),
                l.tint[1].max(0.05),
                l.tint[2].max(0.05),
            ),
            base_color_texture: tex,
            perceptual_roughness: 1.0,
            reflectance: 0.04,
            uv_transform: bevy::math::Affine2::from_scale(
                Vec2::new(l.scale[0][0], l.scale[0][1]) * texscale,
            ),
            unlit: rec.flags & FLAG_NO_LIGHTING != 0 || self.unlit,
            ..default()
        };
        if rec.mask_reference != NONE_U32 {
            m.alpha_mode = AlphaMode::Mask(rec.mask_reference as f32 / 255.0);
            m.double_sided = true;
            m.cull_mode = None;
        }
        let h = mats.add(m);
        self.materials.insert(id, h.clone());
        Some(h)
    }
}

fn stream_cells(
    mut commands: Commands,
    mut a: ResMut<Ardea>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut qmats: ResMut<Assets<QuestMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut phys: ResMut<Physics>,
    mut vrworld: ResMut<VrWorld>,
    mut player: ResMut<VrPlayer>,
) {
    let t0 = Instant::now();
    let budget_ms = 5.0;
    while t0.elapsed().as_secs_f32() * 1e3 < budget_ms {
        let Some((cx, cz)) = a.pending.first().copied() else {
            break;
        };
        a.pending.remove(0);
        let Ok(Some(cell)) = a.cache.load_cell(cx, cz) else {
            continue;
        };
        let mut tris: Vec<[Vec3; 3]> = cell
            .collision_triangles()
            .map(|t| [Vec3::from(t[0]), Vec3::from(t[1]), Vec3::from(t[2])])
            .collect();
        // Static props: the original collision meshes (xnvmsh), transformed by the instance.
        for i in cell.instances() {
            if i.collision == 0 {
                continue;
            }
            let Some(cm) = a.cache.collision(i.collision) else {
                continue;
            };
            let c = i.transform;
            let m = Mat4::from_cols(
                Vec3::from(c[0]).extend(0.0),
                Vec3::from(c[1]).extend(0.0),
                Vec3::from(c[2]).extend(0.0),
                Vec3::from(c[3]).extend(1.0),
            );
            for t in cm.indices.chunks_exact(3) {
                let v = |k: u32| m.transform_point3(Vec3::from(cm.vertices[k as usize]));
                tris.push([v(t[0]), v(t[1]), v(t[2])]);
            }
        }
        vrworld
            .0
            .insert_chunk((((cx as i64) << 32) | (cz as u32 as i64)) as u64, tris);
        let root = a.root;
        let focus = a.focus;
        let (lod_scale, cull_scale) = (a.lod_scale, a.cull_scale);
        let mut jobs: Vec<(u64, Mat4, bool)> = Vec::new();
        for t in cell.terrain() {
            jobs.push((t.mesh, Mat4::IDENTITY, true));
        }
        for i in cell.instances() {
            let c = i.transform;
            let m = Mat4::from_cols(
                Vec3::from(c[0]).extend(0.0),
                Vec3::from(c[1]).extend(0.0),
                Vec3::from(c[2]).extend(0.0),
                Vec3::from(c[3]).extend(1.0),
            );
            jobs.push((i.mesh, m, false));
        }
        for (mesh_id, m, is_terrain) in jobs {
            let Ok(md) = a.cache.load_mesh(AssetId(mesh_id)) else {
                continue;
            };
            let hdr = md.header();
            let (bmin, bmax) = (Vec3::from(hdr.bounds_min), Vec3::from(hdr.bounds_max));
            let scale = m.x_axis.truncate().length().max(1e-3);
            let radius = (bmax - bmin).length() * 0.5 * scale;
            let center = m.transform_point3((bmin + bmax) * 0.5);
            let dist0 = (center - focus).length();
            // Original small-object culling band (render-rules): fully culled beyond `end`.
            let end = if is_terrain {
                f32::MAX
            } else {
                gothic_render_rules::cull::distance_band(
                    radius,
                    gothic_render_rules::cull::screen_cull_factor(0) / cull_scale.max(0.01),
                )
                .1
                .clamp(3_000.0, 25_000.0)
            };
            if dist0 > end + 2_000.0 {
                continue;
            }
            let names = md.material_names();
            for (si, s) in md.submeshes().iter().enumerate() {
                // Distant-LOD proxy atlas submeshes overlap the real geometry (same rule as the desktop loader).
                if names
                    .get(si)
                    .is_some_and(|n| n.to_ascii_lowercase().contains("world_lowpoly"))
                {
                    continue;
                }
                let cheap = a.cheap;
                let (smat, qmat) = if cheap {
                    (None, a.qmaterial(s.material, &mut qmats, &mut images))
                } else {
                    (a.material(s.material, &mut mats, &mut images), None)
                };
                if smat.is_none() && qmat.is_none() {
                    continue;
                }
                let levels = if is_terrain {
                    1
                } else {
                    (s.lod_count as usize).clamp(1, 3)
                };
                let stage_d =
                    gothic_render_rules::lod::stage_distances(radius, levels, lod_scale, 1.0);
                let mut handles: [Option<Handle<Mesh>>; 3] = [None, None, None];
                let mut tris = [0u32; 3];
                for lod in 0..levels {
                    let key = (mesh_id, si as u32, lod as u8);
                    let h = if let Some(h) = a.meshes.get(&key) {
                        h.clone()
                    } else {
                        let (mesh, _) = build_mesh(&md, s, lod);
                        let h = meshes.add(mesh);
                        a.meshes.insert(key, h.clone());
                        h
                    };
                    handles[lod] = Some(h);
                    tris[lod] = md.indices(s, lod).len() as u32 / 3;
                }
                let stage = stage_d
                    .iter()
                    .filter(|d| dist0 > **d)
                    .count()
                    .min(levels - 1);
                let aabb = Aabb::from_min_max(Vec3::from(s.bounds_min), Vec3::from(s.bounds_max));
                let mut ec = commands.spawn((
                    Mesh3d(handles[stage].clone().unwrap()),
                    Transform::from_matrix(m),
                    aabb,
                    TriCount(tris[stage]),
                    StaticProp,
                    Lod {
                        handles,
                        tris,
                        stage_d: [
                            stage_d.first().copied().unwrap_or(f32::MAX),
                            stage_d.get(1).copied().unwrap_or(f32::MAX),
                        ],
                        levels: levels as u8,
                        cur: stage as u8,
                        center,
                        cull_end: end,
                        hidden: false,
                    },
                    ChildOf(root),
                ));
                if let Some(q) = qmat {
                    ec.insert(MeshMaterial3d(q));
                } else if let Some(sm) = smat {
                    ec.insert(MeshMaterial3d(sm));
                }
                a.spawned += 1;
                a.tris += tris[stage] as u64;
            }
        }
        a.cells_done += 1;
    }
    // Release the player once the ground below the start exists.
    if !phys.ready && a.cells_done > 0 {
        let p = player.ctrl.position;
        if let Some(y) = vrworld
            .0
            .ground_below(Vec3::new(p.x, 50_000.0, p.z), 100_000.0)
        {
            player.ctrl.teleport(Vec3::new(p.x, y + 20.0, p.z));
            phys.ready = true;
            info!(
                "[ardea] ground found at y={y:.0} cm after {} cells",
                a.cells_done
            );
        }
    }
}

fn report(
    time: Res<Time>,
    mut acc: Local<f32>,
    a: Res<Ardea>,
    vis: Query<(&ViewVisibility, &TriCount)>,
    player: Res<VrPlayer>,
    frame: Res<gothic_xr_quest::XrFrameData>,
) {
    *acc += time.delta_secs();
    if *acc < 2.0 {
        return;
    }
    *acc = 0.0;
    let (mut n, mut tris) = (0u32, 0u64);
    for (v, t) in &vis {
        if v.get() {
            n += 1;
            tris += t.0 as u64;
        }
    }
    info!(
        "[scene] cells={} pending={} entities={} total_tris={} visible_meshes={} visible_tris={} pos_cm=({:.0},{:.0},{:.0}) grounded={} head_y={:.2} recenters={} loco_us={}",
        a.cells_done,
        a.pending.len(),
        a.spawned,
        a.tris,
        n,
        tris,
        player.ctrl.position.x,
        player.ctrl.position.y,
        player.ctrl.position.z,
        player.ctrl.grounded,
        frame.head.translation.y,
        frame.recenters,
        crate::locomotion::LOCO_US.load(std::sync::atomic::Ordering::Relaxed)
    );
}

/// Host-only (desktop GPUs without ASTC): decode mip 0 of an ASTC 6x6 texture to RGBA8 (same padded size and UV scale).
#[cfg(not(target_os = "android"))]
fn host_astc(t: &TextureData) -> Option<(Image, Vec2)> {
    if !matches!(t.format(), TexFormat::Astc6x6) {
        return None;
    }
    let h = t.header();
    let (pw, ph) = (h.width.div_ceil(6) * 6, h.height.div_ceil(6) * 6);
    let mut px = vec![0u32; (pw * ph) as usize];
    let need = ((pw / 6) * (ph / 6) * 16) as usize;
    texture2ddecoder::decode_astc(
        t.data().get(..need)?,
        pw as usize,
        ph as usize,
        6,
        6,
        &mut px,
    )
    .ok()?;
    let mut rgba = Vec::with_capacity(px.len() * 4);
    for p in px {
        let [b, g, r, a] = p.to_le_bytes();
        rgba.extend_from_slice(&[r, g, b, a]);
    }
    let mut img = Image::new(
        Extent3d {
            width: pw,
            height: ph,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..default()
    });
    Some((
        img,
        Vec2::new(h.width as f32 / pw as f32, h.height as f32 / ph as f32),
    ))
}
