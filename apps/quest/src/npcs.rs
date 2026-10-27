//! Streamed world NPCs: placements, looks and clip names come from the pack's `world_npcs.gnpc`
//! (`examples/pack_world.rs`, selection by `scripts/quest/pack_npcs.py`). NPCs are spawned around the player from the
//! cells the world streamer has resident and play their original idle / routine clip (no AI, no dialogue, no physics).
//!
//! * Assets (skinned meshes, textures, rig data) are built on a worker pool per part variant and shared by every NPC
//!   with that part; attachments (hair, beard) are shared static meshes mounted on a joint.
//! * Skeleton: one flat set of joint entities under the NPC root holding model-space matrices, evaluated from the clip
//!   pose with `gothic_animation::rig::evaluate` (the same evaluation the desktop animator uses).
//! * Animation update rate by distance: near 50 Hz, mid 35 Hz, far 10 Hz, every NPC with its own random phase.
//!   NPCs behind the head or beyond the render radius are hidden (no skinning, no animation); beyond the spawn radius
//!   they are despawned. Lighter original actor LODs (`*_lod1/2`) are swapped in by distance when the pack has them.
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::math::Affine3A;
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use gothic_animation::clip::{Clip, ClipBinding};
use gothic_animation::clip_pack::ClipPack;
use gothic_animation::pose::Trs;
use gothic_animation::rig::{self, EvalStep, Rig};
use gothic_cook::actor::ActorNodeRec;
use gothic_cook::{AssetId, Cache};
use gothic_vr::{VrPlayer, VrWorld};
use gothic_xr_quest::knob_f32;

use crate::ardea::{Ardea, build_image, mirror_mesh_x};
use crate::qmat::{QParams, QuestMaterial};
use crate::world::{Workers, WorldStream, cell_at};

const CM: f32 = 100.0;

// ---------------------------------------------------------------------------------------------------------------------
// Manifest
// ---------------------------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
pub struct SubOv {
    pub tex: u64,
    pub kind: u8,
    pub mask: u8,
}

pub struct Variant {
    pub actor: String,
    pub lod: [u32; 2],
    pub subs: Vec<SubOv>,
}

pub struct AttachRec {
    pub mesh: String,
    pub joint: String,
    pub offset: [f32; 12],
    pub ov: SubOv,
}

pub struct LookRec {
    pub family: String,
    pub fix: Quat,
    pub parts: Vec<u32>,
    pub att: Vec<AttachRec>,
}

pub struct NpcRec {
    /// World-root centimetres (Bevy x = -Gothic x), original height.
    pub pos: Vec3,
    pub yaw: f32,
    pub look: u32,
    pub idle: String,
    pub routine: Option<String>,
    pub species: String,
}

pub struct Manifest {
    pub variants: Vec<Variant>,
    pub looks: Vec<LookRec>,
    pub npcs: Vec<NpcRec>,
    pub by_cell: HashMap<(i32, i32), Vec<u32>>,
}

struct Rd<'a>(&'a [u8], usize);
impl Rd<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.0.get(self.1..self.1 + n)?;
        self.1 += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    fn f32(&mut self) -> Option<f32> {
        Some(f32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
}

impl Manifest {
    pub fn load(path: &Path) -> Option<Self> {
        let data = std::fs::read(path).ok()?;
        if !data.starts_with(b"GNPC0001") {
            return None;
        }
        let mut r = Rd(&data, 8);
        let ns = r.u32()? as usize;
        let mut strings = Vec::with_capacity(ns);
        for _ in 0..ns {
            let n = u16::from_le_bytes(r.take(2)?.try_into().ok()?) as usize;
            strings.push(String::from_utf8_lossy(r.take(n)?).into_owned());
        }
        let s = |i: u32| strings.get(i as usize).cloned().unwrap_or_default();
        let nv = r.u32()? as usize;
        let mut variants = Vec::with_capacity(nv);
        for _ in 0..nv {
            let actor = s(r.u32()?);
            let lod = [r.u32()?, r.u32()?];
            let n = r.u32()? as usize;
            let mut subs = Vec::with_capacity(n);
            for _ in 0..n {
                let tex = r.u64()?;
                let (kind, mask) = (r.u8()?, r.u8()?);
                r.take(2)?;
                subs.push(SubOv { tex, kind, mask });
            }
            variants.push(Variant { actor, lod, subs });
        }
        let nl = r.u32()? as usize;
        let mut looks = Vec::with_capacity(nl);
        for _ in 0..nl {
            let family = s(r.u32()?);
            let fix = Quat::from_xyzw(r.f32()?, r.f32()?, r.f32()?, r.f32()?);
            let np = r.u32()? as usize;
            let mut parts = Vec::with_capacity(np);
            for _ in 0..np {
                parts.push(r.u32()?);
            }
            let na = r.u32()? as usize;
            let mut att = Vec::with_capacity(na);
            for _ in 0..na {
                let mesh = s(r.u32()?);
                let joint = s(r.u32()?);
                let mut offset = [0.0; 12];
                for o in &mut offset {
                    *o = r.f32()?;
                }
                let tex = r.u64()?;
                let (kind, mask) = (r.u8()?, r.u8()?);
                r.take(2)?;
                att.push(AttachRec {
                    mesh,
                    joint,
                    offset,
                    ov: SubOv { tex, kind, mask },
                });
            }
            looks.push(LookRec {
                family,
                fix,
                parts,
                att,
            });
        }
        let nn = r.u32()? as usize;
        let mut npcs = Vec::with_capacity(nn);
        let mut by_cell: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for i in 0..nn {
            let pos = Vec3::new(r.f32()?, r.f32()?, r.f32()?);
            let yaw = r.f32()?;
            let look = r.u32()?;
            let idle = s(r.u32()?);
            let routine = r.u32()?;
            let species = s(r.u32()?);
            let _tier = r.u32()?;
            by_cell.entry(cell_at(pos)).or_default().push(i as u32);
            npcs.push(NpcRec {
                pos,
                yaw,
                look,
                idle,
                routine: (routine != u32::MAX).then(|| s(routine)),
                species,
            });
        }
        Some(Self {
            variants,
            looks,
            npcs,
            by_cell,
        })
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Worker-side part building (cooked actor -> CPU data)
// ---------------------------------------------------------------------------------------------------------------------

fn node_local(n: &ActorNodeRec) -> Transform {
    Transform {
        translation: Vec3::from(gothic_coords::arr3(n.position)),
        rotation: gothic_coords::quat_xyzw(n.rotation),
        scale: Vec3::from(n.scale),
    }
}

struct SubCpu {
    mesh: Mesh,
    skinned: bool,
    node: usize,
    /// Texture id and the layer UV scale it is drawn with (before the padded-block correction).
    tex: Option<(u64, Vec2)>,
    tint: Vec3,
    cutoff: f32,
}

struct PartCpu {
    names: Vec<String>,
    parent: Vec<i32>,
    bind_local: Vec<Transform>,
    part_global: Vec<Mat4>,
    subs: Vec<SubCpu>,
    images: Vec<(u64, Image, Vec2)>,
    bytes: u64,
}

fn global_matrices(nodes: &[ActorNodeRec]) -> Vec<Mat4> {
    let n = nodes.len();
    let mut g = vec![Mat4::IDENTITY; n];
    let mut done = vec![false; n];
    let mut left = n;
    while left > 0 {
        let before = left;
        for (i, rec) in nodes.iter().enumerate() {
            if done[i] {
                continue;
            }
            let l = node_local(rec).to_matrix();
            if rec.parent < 0 {
                g[i] = l;
            } else if done[rec.parent as usize] {
                g[i] = g[rec.parent as usize] * l;
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
    g
}

fn build_part(cache: &Cache, actor: &str, subs_ov: &[SubOv]) -> Option<PartCpu> {
    let data = cache.load_actor(AssetId::of(actor)).ok()?;
    let names = data.node_names();
    let nodes = data.nodes();
    let part_global = global_matrices(nodes);
    let amats = data.materials();
    let verts = data.vertices();
    let idx = data.indices();
    let mut subs = Vec::new();
    let mut want: Vec<u64> = Vec::new();
    let mut bytes = 0u64;
    for (k, sub) in data.submeshes().iter().enumerate() {
        let vs = &verts[sub.first_vertex as usize..(sub.first_vertex + sub.vertex_count) as usize];
        let is = &idx[sub.first_index as usize..(sub.first_index + sub.index_count) as usize];
        let mut m = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
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
        bytes += vs.len() as u64 * 56 + is.len() as u64 * 4;
        let am = amats.get(sub.material as usize);
        let ov = subs_ov.get(k).copied().unwrap_or_default();
        // Texture: the library's resolved (material-switched) diffuse, else the cooked actor material's diffuse, else
        // the diffuse layer of its shader material (heads keep the skin there).
        let mut tex: Option<(u64, Vec2)> = (ov.tex != 0).then_some((ov.tex, Vec2::ONE));
        let mut from_tex = tex.is_some();
        if tex.is_none() {
            if let Some(am) = am {
                if am.diffuse_tex != 0 {
                    tex = Some((am.diffuse_tex, Vec2::ONE));
                    from_tex = true;
                } else if let Some(rec) = cache.material(am.shader_material) {
                    let l = rec.layers[0];
                    if l.tex[0] != 0 {
                        tex = Some((l.tex[0], Vec2::new(l.scale[0][0], l.scale[0][1])));
                        from_tex = true;
                    }
                }
            }
        }
        if let Some((id, _)) = tex {
            want.push(id);
        }
        let tint = if from_tex {
            Vec3::ONE
        } else {
            am.map_or(Vec3::splat(0.6), |am| {
                Vec3::new(am.diffuse[0], am.diffuse[1], am.diffuse[2])
            })
        };
        let cutoff = if ov.kind == 1 {
            f32::from(ov.mask.max(1)) / 255.0
        } else {
            0.0
        };
        subs.push(SubCpu {
            mesh: m,
            skinned: sub.skinned != 0,
            node: sub.node as usize,
            tex,
            tint,
            cutoff,
        });
    }
    want.sort_unstable();
    want.dedup();
    let mut images = Vec::new();
    for id in want {
        if let Some((img, uv)) = cache
            .load_texture(AssetId(id))
            .ok()
            .and_then(|t| build_image(&t))
        {
            bytes += img.data.as_ref().map_or(0, |d| d.len() as u64);
            images.push((id, img, uv));
        }
    }
    Some(PartCpu {
        names,
        parent: nodes.iter().map(|n| n.parent).collect(),
        bind_local: nodes.iter().map(node_local).collect(),
        part_global,
        subs,
        images,
        bytes,
    })
}

struct AttachCpu {
    subs: Vec<(Mesh, Option<(u64, Vec2)>, Vec3, f32)>,
    images: Vec<(u64, Image, Vec2)>,
}

fn build_attach(cache: &Cache, mesh: &str, ov: SubOv) -> Option<AttachCpu> {
    let md = cache.load_mesh(AssetId::of(mesh)).ok()?;
    let mut subs = Vec::new();
    let mut want: Vec<u64> = Vec::new();
    for s in md.submeshes() {
        let rec = cache.material(s.material);
        let l = rec.map(|r| r.layers[0]);
        let mut tex = (ov.tex != 0).then_some((ov.tex, Vec2::new(1.0, 1.0)));
        if tex.is_none() {
            if let Some(l) = l {
                if l.tex[0] != 0 {
                    tex = Some((l.tex[0], Vec2::new(l.scale[0][0], l.scale[0][1])));
                }
            }
        }
        if let Some((id, _)) = tex {
            want.push(id);
        }
        let tint = l.map_or(Vec3::splat(0.5), |l| {
            Vec3::new(l.tint[0].max(0.05), l.tint[1].max(0.05), l.tint[2].max(0.05))
        });
        let cutoff = if ov.kind == 1 {
            f32::from(ov.mask.max(1)) / 255.0
        } else {
            rec.filter(|r| r.mask_reference != gothic_cook::material::NONE_U32)
                .map_or(0.0, |r| (r.mask_reference as f32 / 255.0).max(0.05))
        };
        let vs = md.submesh_vertices(s);
        let mut m = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        );
        m.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vs.iter().map(|v| v.pos).collect::<Vec<_>>(),
        );
        m.insert_attribute(
            Mesh::ATTRIBUTE_NORMAL,
            vs.iter().map(|v| v.normal).collect::<Vec<_>>(),
        );
        m.insert_attribute(Mesh::ATTRIBUTE_UV_0, vs.iter().map(|v| v.uv).collect::<Vec<_>>());
        m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1.0f32; 4]; vs.len()]);
        m.insert_indices(match md.indices(s, 0) {
            gothic_cook::mesh::Indices::U16(i) => Indices::U16(i.to_vec()),
            gothic_cook::mesh::Indices::U32(i) => Indices::U32(i.to_vec()),
        });
        mirror_mesh_x(&mut m);
        subs.push((m, tex, tint, cutoff));
    }
    want.sort_unstable();
    want.dedup();
    let images = want
        .into_iter()
        .filter_map(|id| {
            cache
                .load_texture(AssetId(id))
                .ok()
                .and_then(|t| build_image(&t))
                .map(|(i, uv)| (id, i, uv))
        })
        .collect();
    Some(AttachCpu { subs, images })
}

enum NpcPayload {
    Part(u32, Option<PartCpu>),
    Attach(u64, Option<AttachCpu>),
}

// ---------------------------------------------------------------------------------------------------------------------
// Main-thread assets
// ---------------------------------------------------------------------------------------------------------------------

struct PartSubAsset {
    mesh: Handle<Mesh>,
    mat: Handle<QuestMaterial>,
    skinned: bool,
    node: usize,
}

struct PartAsset {
    names: Vec<String>,
    part_global: Vec<Mat4>,
    ibp: Handle<SkinnedMeshInverseBindposes>,
    subs: Vec<PartSubAsset>,
    /// Rig data (used for the body part).
    parent: Vec<i32>,
    bind_local: Vec<Transform>,
}

struct AttachAsset {
    subs: Vec<(Handle<Mesh>, Handle<QuestMaterial>)>,
}

/// Skeleton evaluation shared by every NPC with the same body actor.
struct RigSet {
    rig: Arc<Rig>,
    plan: Vec<EvalStep>,
    names: Vec<String>,
    by_name_lc: HashMap<String, usize>,
    bind_model: Vec<Affine3A>,
}

#[derive(Resource)]
pub struct Npcs {
    pub manifest: Manifest,
    cache: Arc<Cache>,
    workers: Workers,
    clips: HashMap<String, Arc<Clip>>,
    variants: HashMap<u32, Option<Arc<PartAsset>>>,
    variant_pending: HashSet<u32>,
    attach: HashMap<u64, Option<Arc<AttachAsset>>>,
    attach_pending: HashSet<u64>,
    rigsets: HashMap<String, Arc<RigSet>>,
    bindings: HashMap<(String, String), Arc<ClipBinding>>,
    spawned: HashMap<u32, Entity>,
    failed: HashSet<u32>,
    spawn_r: f32,
    despawn_r: f32,
    render_r: f32,
    near_r: f32,
    mid_r: f32,
    lod1_r: f32,
    lod2_r: f32,
    max_npcs: usize,
    pub stats: NpcStats,
}

impl Npcs {
    /// Manifest positions of the NPCs that are spawned right now.
    pub fn live_positions(&self) -> Vec<Vec3> {
        self.spawned
            .keys()
            .map(|&i| self.manifest.npcs[i as usize].pos)
            .collect()
    }
}

#[derive(Default, Clone, Debug)]
pub struct NpcStats {
    pub live: usize,
    pub visible: usize,
    pub animated_last: usize,
    pub anim_us_last: u32,
    pub anim_us_peak: u32,
    pub manage_us_last: u32,
    pub manage_us_peak: u32,
    pub spawned_total: u64,
    pub skin_tris_visible: u64,
}

#[derive(Component)]
pub struct Npc {
    idx: u32,
    rigset: Arc<RigSet>,
    joints: Vec<Entity>,
    clip: Arc<Clip>,
    binding: Arc<ClipBinding>,
    phase: f32,
    next: f32,
    lod: u8,
    meshes: Vec<Entity>,
    visible: bool,
    dist: f32,
}

pub fn clips_path(dir: &Path) -> std::path::PathBuf {
    dir.join("clips_npc.grcp")
}

/// Registers the NPC systems when the pack has an NPC manifest (called by `lib.rs` after `ardea::build`).
pub fn install(app: &mut App, dir: &Path) {
    if gothic_xr_quest::knob("npcs").as_deref() == Some("0") {
        return;
    }
    let Some(manifest) = Manifest::load(&dir.join("world_npcs.gnpc")) else {
        log::info!("[npcs] no world_npcs.gnpc in the pack");
        return;
    };
    let Ok(pack) = ClipPack::load(&clips_path(dir)) else {
        log::warn!("[npcs] clips_npc.grcp missing");
        return;
    };
    let clips: HashMap<String, Arc<Clip>> = pack
        .clips
        .into_iter()
        .map(|c| (c.clip.name.clone(), Arc::new(c.clip)))
        .collect();
    let cache = app.world().resource::<Ardea>().cache.clone();
    log::info!(
        "[npcs] manifest: {} placements, {} looks, {} part variants, {} clips",
        manifest.npcs.len(),
        manifest.looks.len(),
        manifest.variants.len(),
        clips.len()
    );
    let spawn_r = knob_f32("npc_spawn", 9_000.0);
    app.insert_resource(Npcs {
        manifest,
        cache,
        workers: Workers::new(knob_f32("npc_threads", 2.0) as usize, "npc-io"),
        clips,
        variants: HashMap::new(),
        variant_pending: HashSet::new(),
        attach: HashMap::new(),
        attach_pending: HashSet::new(),
        rigsets: HashMap::new(),
        bindings: HashMap::new(),
        spawned: HashMap::new(),
        failed: HashSet::new(),
        spawn_r,
        despawn_r: spawn_r * 1.25,
        render_r: knob_f32("npc_render", 7_000.0),
        near_r: knob_f32("npc_near", 1_500.0),
        mid_r: knob_f32("npc_mid", 3_500.0),
        lod1_r: knob_f32("npc_lod1", 2_500.0),
        lod2_r: knob_f32("npc_lod2", 5_500.0),
        max_npcs: knob_f32("npc_max", 80.0) as usize,
        stats: NpcStats::default(),
    });
    app.add_systems(
        Update,
        (npc_manage, npc_animate.after(npc_manage), npc_report),
    );
}

fn norm(s: &str) -> String {
    s.to_ascii_lowercase()
}

fn quest_material(
    a: &mut Ardea,
    qmats: &mut Assets<QuestMaterial>,
    images: &HashMap<u64, Vec2>,
    tex: Option<(u64, Vec2)>,
    tint: Vec3,
    cutoff: f32,
) -> Handle<QuestMaterial> {
    let (h, uv) = match tex.and_then(|(id, mul)| {
        let pad = images.get(&id).copied().or_else(|| {
            a.textures
                .get(&id)
                .and_then(|t| t.as_ref())
                .map(|t| t.1)
        })?;
        let handle = a.textures.get(&id)?.as_ref()?.0.clone();
        Some((handle, mul * pad))
    }) {
        Some(t) => (Some(t.0), t.1),
        None => (Some(a.white_image()), Vec2::ONE),
    };
    let tint = if h.as_ref() == Some(&a.white_image()) && tex.is_some() {
        Vec3::splat(0.5)
    } else {
        tint
    };
    qmats.add(QuestMaterial {
        params: QParams::new(tint, uv, cutoff),
        tex: h,
        tex_b: None,
        detail_a: None,
        detail_b: None,
        two_sided: true,
    })
}

#[allow(clippy::too_many_arguments)]
fn npc_manage(
    mut commands: Commands,
    mut n: ResMut<Npcs>,
    ws: Res<WorldStream>,
    mut a: ResMut<Ardea>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut qmats: ResMut<Assets<QuestMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut inv: ResMut<Assets<SkinnedMeshInverseBindposes>>,
    world: Res<VrWorld>,
    player: Res<VrPlayer>,
    eyes: Query<&GlobalTransform, With<gothic_xr_quest::XrEye>>,
    mut live: Query<&mut Npc>,
) {
    let t0 = Instant::now();
    let n = &mut *n;
    let pp = player.ctrl.position;
    let eye = eyes
        .iter()
        .next()
        .map_or(pp, |e| e.translation() * CM);

    // 1. Finished asset jobs.
    for d in n.workers.drain(32) {
        let Ok(p) = d.payload.downcast::<NpcPayload>() else {
            continue;
        };
        match *p {
            NpcPayload::Part(v, cpu) => {
                n.variant_pending.remove(&v);
                let asset = cpu.map(|cpu| {
                    let mut pad: HashMap<u64, Vec2> = HashMap::new();
                    for (id, img, uv) in cpu.images {
                        pad.insert(id, uv);
                        a.textures
                            .entry(id)
                            .or_insert_with(|| Some((images.add(img), uv)));
                    }
                    let ibp = inv.add(SkinnedMeshInverseBindposes::from(
                        cpu.part_global
                            .iter()
                            .map(|m| m.inverse())
                            .collect::<Vec<_>>(),
                    ));
                    let subs = cpu
                        .subs
                        .into_iter()
                        .map(|s| PartSubAsset {
                            mat: quest_material(&mut a, &mut qmats, &pad, s.tex, s.tint, s.cutoff),
                            mesh: meshes.add(s.mesh),
                            skinned: s.skinned,
                            node: s.node,
                        })
                        .collect();
                    Arc::new(PartAsset {
                        names: cpu.names,
                        part_global: cpu.part_global,
                        ibp,
                        subs,
                        parent: cpu.parent,
                        bind_local: cpu.bind_local,
                    })
                });
                n.variants.insert(v, asset);
            }
            NpcPayload::Attach(key, cpu) => {
                n.attach_pending.remove(&key);
                let asset = cpu.map(|cpu| {
                    let mut pad: HashMap<u64, Vec2> = HashMap::new();
                    for (id, img, uv) in cpu.images {
                        pad.insert(id, uv);
                        a.textures
                            .entry(id)
                            .or_insert_with(|| Some((images.add(img), uv)));
                    }
                    Arc::new(AttachAsset {
                        subs: cpu
                            .subs
                            .into_iter()
                            .map(|(m, tex, tint, cutoff)| {
                                (
                                    meshes.add(m),
                                    quest_material(&mut a, &mut qmats, &pad, tex, tint, cutoff),
                                )
                            })
                            .collect(),
                    })
                });
                n.attach.insert(key, asset);
            }
        }
    }

    // 2. Despawn NPCs that are far away.
    let far: Vec<u32> = n
        .spawned
        .iter()
        .filter_map(|(&i, _)| {
            let d = n.manifest.npcs[i as usize].pos.xz().distance(pp.xz());
            (d > n.despawn_r).then_some(i)
        })
        .collect();
    for i in far {
        if let Some(e) = n.spawned.remove(&i) {
            commands.entity(e).despawn();
        }
    }

    // 3. Candidates in range inside resident cells with collision, nearest first.
    let (pcx, pcz) = cell_at(pp);
    let reach = (n.spawn_r / 10_000.0).ceil() as i32;
    let mut cand: Vec<(f32, u32)> = Vec::new();
    for dx in -reach..=reach {
        for dz in -reach..=reach {
            let k = (pcx + dx, pcz + dz);
            let Some(list) = n.manifest.by_cell.get(&k) else {
                continue;
            };
            if !ws.resident(k.0, k.1) || !ws.has_collision(k.0, k.1) {
                continue;
            }
            for &i in list {
                if n.spawned.contains_key(&i) || n.failed.contains(&i) {
                    continue;
                }
                let d = n.manifest.npcs[i as usize].pos.xz().distance(pp.xz());
                if d <= n.spawn_r {
                    cand.push((d, i));
                }
            }
        }
    }
    cand.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut spawned_now = 0;
    for (_, i) in cand {
        if n.spawned.len() >= n.max_npcs || spawned_now >= 2 || t0.elapsed().as_micros() > 1_500 {
            break;
        }
        let (look_i, pos, yaw, idle, routine) = {
            let r = &n.manifest.npcs[i as usize];
            (r.look, r.pos, r.yaw, r.idle.clone(), r.routine.clone())
        };
        // Needed assets: LOD0 variants of every part and the attachments.
        let (parts, atts, family): (Vec<u32>, Vec<(u64, String, SubOv)>, String) = {
            let look = &n.manifest.looks[look_i as usize];
            (
                look.parts.clone(),
                look.att
                    .iter()
                    .map(|at| (attach_key(&at.mesh, at.ov), at.mesh.clone(), at.ov))
                    .collect(),
                look.family.clone(),
            )
        };
        let mut ready = true;
        for &v in &parts {
            if !n.variants.contains_key(&v) && !n.variant_pending.contains(&v) {
                request_part(n, v, pos.xz().distance(pp.xz()));
            }
            ready &= n.variants.contains_key(&v);
        }
        for (key, mesh, ov) in &atts {
            if !n.attach.contains_key(key) && !n.attach_pending.contains(key) {
                request_attach(n, *key, mesh, *ov);
            }
            ready &= n.attach.contains_key(key);
        }
        if !ready {
            continue;
        }
        if n.variants[&parts[0]].is_none() {
            n.failed.insert(i);
            continue;
        }
        let body = n.variants[&parts[0]].clone().unwrap();
        let body_actor = norm(&n.manifest.variants[parts[0] as usize].actor);
        let clip_name = routine.unwrap_or(idle);
        let Some(clip) = n.clips.get(&clip_name).cloned() else {
            n.failed.insert(i);
            continue;
        };
        let rigset = match n.rigsets.get(&body_actor) {
            Some(r) => r.clone(),
            None => {
                let r = Arc::new(make_rigset(&body, &family));
                n.rigsets.insert(body_actor.clone(), r.clone());
                r
            }
        };
        let bkey = (body_actor.clone(), clip_name.clone());
        let binding = match n.bindings.get(&bkey) {
            Some(b) => b.clone(),
            None => {
                let b = Arc::new(ClipBinding::new(&rigset.rig, &clip));
                n.bindings.insert(bkey, b.clone());
                b
            }
        };
        // Ground: the surface nearest the original height (never a roof): small window first, then further down.
        let from = Vec3::new(pos.x, pos.y + 50.0, pos.z);
        let ground = world
            .0
            .ground_below(from, 100.0)
            .or_else(|| world.0.ground_below(from, 800.0))
            .unwrap_or(pos.y);
        let phase = hash01(i, 3) * clip.duration.max(0.1);
        let root = commands
            .spawn((
                Transform {
                    translation: Vec3::new(pos.x, ground, pos.z),
                    rotation: Quat::from_rotation_y(yaw) * n.manifest.looks[look_i as usize].fix,
                    scale: Vec3::ONE,
                },
                Visibility::Hidden,
                ChildOf(a.root),
                Name::new(format!("npc {i}")),
            ))
            .id();
        // Flat joint entities with the bind pose.
        let joints: Vec<Entity> = rigset
            .bind_model
            .iter()
            .map(|m| {
                let (s, r, t) = m.to_scale_rotation_translation();
                commands
                    .spawn((
                        Transform {
                            translation: t,
                            rotation: r,
                            scale: s,
                        },
                        Visibility::Inherited,
                        ChildOf(root),
                    ))
                    .id()
            })
            .collect();
        let mut mesh_ents = Vec::new();
        spawn_parts(
            &mut commands,
            n,
            &rigset,
            &joints,
            root,
            &parts,
            0,
            &mut mesh_ents,
        );
        for at in &n.manifest.looks[look_i as usize].att {
            let key = attach_key(&at.mesh, at.ov);
            let Some(Some(asset)) = n.attach.get(&key) else {
                continue;
            };
            let Some(&j) = rigset.by_name_lc.get(&norm(&at.joint)) else {
                continue;
            };
            let o = &at.offset;
            let m = Mat4::from_cols(
                Vec3::new(o[0], o[1], o[2]).extend(0.0),
                Vec3::new(o[3], o[4], o[5]).extend(0.0),
                Vec3::new(o[6], o[7], o[8]).extend(0.0),
                Vec3::new(o[9], o[10], o[11]).extend(1.0),
            );
            for (mh, mat) in &asset.subs {
                mesh_ents.push(
                    commands
                        .spawn((
                            Mesh3d(mh.clone()),
                            MeshMaterial3d(mat.clone()),
                            Transform::from_matrix(m),
                            NoFrustumCulling,
                            ChildOf(joints[j]),
                        ))
                        .id(),
                );
            }
        }
        commands.entity(root).insert(Npc {
            idx: i,
            rigset,
            joints,
            clip,
            binding,
            phase,
            next: 0.0,
            lod: 0,
            meshes: mesh_ents,
            visible: false,
            dist: 0.0,
        });
        n.spawned.insert(i, root);
        n.stats.spawned_total += 1;
        spawned_now += 1;
    }

    // 4. Mesh LOD swaps (original lighter actor LODs) by distance, with 10 % hysteresis.
    let mut swaps: Vec<(Entity, u8)> = Vec::new();
    for npc in live.iter() {
        let d = npc.dist;
        let want = if d > n.lod2_r {
            2
        } else if d > n.lod1_r {
            1
        } else {
            0
        };
        let keep = (npc.lod == 1 && want == 0 && d > n.lod1_r * 0.9)
            || (npc.lod == 2 && want == 1 && d > n.lod2_r * 0.9);
        if want != npc.lod && !keep && npc.visible {
            swaps.push((n.spawned[&npc.idx], want));
        }
    }
    for (e, want) in swaps.into_iter().take(2) {
        let Ok(mut npc) = live.get_mut(e) else {
            continue;
        };
        let look = &n.manifest.looks[n.manifest.npcs[npc.idx as usize].look as usize];
        let parts: Vec<u32> = look.parts.clone();
        // Every part's stage variant must be loaded (variants without a lighter LOD keep their own stage).
        let stage_of = |n: &Npcs, v: u32| -> u32 {
            let var = &n.manifest.variants[v as usize];
            match want {
                2 if var.lod[1] != 0 => var.lod[1] - 1,
                2 | 1 if var.lod[0] != 0 => var.lod[0] - 1,
                _ => v,
            }
        };
        let mut ok = true;
        for &p in &parts {
            let s = stage_of(n, p);
            if !n.variants.contains_key(&s) {
                if !n.variant_pending.contains(&s) {
                    request_part(n, s, npc.dist);
                }
                ok = false;
            }
        }
        if !ok {
            continue;
        }
        for me in npc.meshes.drain(..) {
            commands.entity(me).despawn();
        }
        // Re-spawn: attachments stay at LOD0 (tiny), skinned parts use the stage variants.
        let stage_parts: Vec<u32> = parts.iter().map(|&p| stage_of(n, p)).collect();
        let rigset = npc.rigset.clone();
        let joints = npc.joints.clone();
        let mut mesh_ents = Vec::new();
        spawn_parts(
            &mut commands,
            n,
            &rigset,
            &joints,
            e,
            &stage_parts,
            want,
            &mut mesh_ents,
        );
        for at in &n.manifest.looks[n.manifest.npcs[npc.idx as usize].look as usize].att {
            let key = attach_key(&at.mesh, at.ov);
            let Some(Some(asset)) = n.attach.get(&key) else {
                continue;
            };
            let Some(&j) = rigset.by_name_lc.get(&norm(&at.joint)) else {
                continue;
            };
            let o = &at.offset;
            let m = Mat4::from_cols(
                Vec3::new(o[0], o[1], o[2]).extend(0.0),
                Vec3::new(o[3], o[4], o[5]).extend(0.0),
                Vec3::new(o[6], o[7], o[8]).extend(0.0),
                Vec3::new(o[9], o[10], o[11]).extend(1.0),
            );
            for (mh, mat) in &asset.subs {
                mesh_ents.push(
                    commands
                        .spawn((
                            Mesh3d(mh.clone()),
                            MeshMaterial3d(mat.clone()),
                            Transform::from_matrix(m),
                            NoFrustumCulling,
                            ChildOf(joints[j]),
                        ))
                        .id(),
                );
            }
        }
        npc.meshes = mesh_ents;
        npc.lod = want;
    }
    let _ = eye;
    let us = t0.elapsed().as_micros() as u32;
    n.stats.manage_us_last = us;
    n.stats.manage_us_peak = n.stats.manage_us_peak.max(us);
}

fn hash01(i: u32, salt: u64) -> f32 {
    let mut x = u64::from(i).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ salt.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 30;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 27;
    (x >> 40) as f32 / (1u64 << 24) as f32
}

fn attach_key(mesh: &str, ov: SubOv) -> u64 {
    AssetId::of(&format!("{mesh}|{}|{}|{}", ov.tex, ov.kind, ov.mask)).0
}

fn request_part(n: &mut Npcs, v: u32, dist: f32) {
    n.variant_pending.insert(v);
    let (cache, actor, subs) = {
        let var = &n.manifest.variants[v as usize];
        (n.cache.clone(), var.actor.clone(), var.subs.clone_subs())
    };
    n.workers
        .submit(dist, Arc::new(AtomicBool::new(false)), 0, move || {
            Box::new(NpcPayload::Part(v, build_part(&cache, &actor, &subs)))
        });
}

trait CloneSubs {
    fn clone_subs(&self) -> Vec<SubOv>;
}
impl CloneSubs for Vec<SubOv> {
    fn clone_subs(&self) -> Vec<SubOv> {
        self.clone()
    }
}

fn request_attach(n: &mut Npcs, key: u64, mesh: &str, ov: SubOv) {
    n.attach_pending.insert(key);
    let (cache, mesh) = (n.cache.clone(), mesh.to_string());
    n.workers
        .submit(0.0, Arc::new(AtomicBool::new(false)), 1, move || {
            Box::new(NpcPayload::Attach(key, build_attach(&cache, &mesh, ov)))
        });
}

fn make_rigset(body: &PartAsset, family: &str) -> RigSet {
    let parent: Vec<u16> = body
        .parent
        .iter()
        .map(|&p| if p < 0 { rig::NONE } else { p as u16 })
        .collect();
    let bind: Vec<Trs> = body
        .bind_local
        .iter()
        .map(|t| Trs {
            t: t.translation,
            r: t.rotation,
            s: t.scale,
        })
        .collect();
    let mut rg = Rig::from_parts(family, body.names.clone(), parent, bind);
    rg.family = family.to_string();
    let rg = Arc::new(rg);
    let plan = rg.plan(&(0..rg.names.len() as u16).collect::<Vec<_>>());
    let mut model = vec![Affine3A::IDENTITY; rg.names.len()];
    let pose = rg.rest.clone();
    rig::evaluate(&plan, &pose, &mut model);
    RigSet {
        by_name_lc: body
            .names
            .iter()
            .enumerate()
            .map(|(i, n)| (norm(n), i))
            .collect(),
        names: body.names.clone(),
        bind_model: model,
        plan,
        rig: rg,
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_parts(
    commands: &mut Commands,
    n: &Npcs,
    rs: &RigSet,
    joints: &[Entity],
    root: Entity,
    parts: &[u32],
    _lod: u8,
    out: &mut Vec<Entity>,
) {
    for &v in parts {
        let Some(Some(asset)) = n.variants.get(&v) else {
            continue;
        };
        let joint_entities: Vec<Entity> = asset
            .names
            .iter()
            .map(|nm| rs.by_name_lc.get(&norm(nm)).map_or(root, |&i| joints[i]))
            .collect();
        for s in &asset.subs {
            let mut e = commands.spawn((
                Mesh3d(s.mesh.clone()),
                MeshMaterial3d(s.mat.clone()),
                Transform::IDENTITY,
                NoFrustumCulling,
            ));
            if s.skinned {
                e.insert((
                    SkinnedMesh {
                        inverse_bindposes: asset.ibp.clone(),
                        joints: joint_entities.clone(),
                    },
                    ChildOf(root),
                ));
            } else if s.node < joint_entities.len() {
                e.insert((
                    Transform::from_matrix(asset.part_global[s.node].inverse()),
                    ChildOf(joint_entities[s.node]),
                ));
            } else {
                e.insert(ChildOf(root));
            }
            out.push(e.id());
        }
    }
    let _ = &rs.names;
}

// ---------------------------------------------------------------------------------------------------------------------
// Animation
// ---------------------------------------------------------------------------------------------------------------------

#[derive(Default)]
struct Scratch {
    pose: Vec<Trs>,
    model: Vec<Affine3A>,
}

#[allow(clippy::too_many_arguments)]
fn npc_animate(
    time: Res<Time>,
    mut n: ResMut<Npcs>,
    player: Res<VrPlayer>,
    eyes: Query<&GlobalTransform, With<gothic_xr_quest::XrEye>>,
    mut q: Query<(&mut Npc, &mut Visibility)>,
    mut joints: Query<&mut Transform, Without<Npc>>,
    mut scratch: Local<Scratch>,
) {
    let t0 = Instant::now();
    let t = time.elapsed_secs();
    let (eye_pos, fwd) = eyes
        .iter()
        .next()
        .map_or((player.ctrl.position, Vec3::NEG_Z), |e| {
            (e.translation() * CM, *e.forward())
        });
    let fwd_h = Vec3::new(fwd.x, 0.0, fwd.z).normalize_or_zero();
    let (render_r, near_r, mid_r) = (n.render_r, n.near_r, n.mid_r);
    let (mut live, mut visible, mut animated, mut tris) = (0, 0, 0, 0u64);
    let npcs = &n.manifest.npcs;
    for (mut npc, mut vis) in &mut q {
        live += 1;
        let pos = npcs[npc.idx as usize].pos;
        let dv = Vec3::new(pos.x - eye_pos.x, 0.0, pos.z - eye_pos.z);
        let d = dv.length();
        npc.dist = d;
        // Behind the head (generous cone: the eye FOV is ~100 deg, the head turns between frames) or too far.
        let in_view = d < 400.0 || fwd_h == Vec3::ZERO || dv.dot(fwd_h) > d * 0.17;
        let show = d < render_r && in_view;
        if show != npc.visible {
            npc.visible = show;
            *vis = if show {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            npc.next = 0.0;
        }
        if !show {
            continue;
        }
        visible += 1;
        tris += 6000;
        if t < npc.next {
            continue;
        }
        let rate = if d < near_r {
            50.0
        } else if d < mid_r {
            35.0
        } else {
            10.0
        };
        npc.next = t + 1.0 / rate;
        animated += 1;
        // Sample, evaluate, write the joint matrices.
        let rs = npc.rigset.clone();
        let nslots = rs.rig.rest.len();
        scratch.pose.resize(nslots, Trs::IDENTITY);
        scratch.model.resize(rs.names.len(), Affine3A::IDENTITY);
        let tt = npc.clip.wrap(t + npc.phase);
        let Scratch { pose, model } = &mut *scratch;
        npc.binding.sample(&npc.clip, &rs.rig.rest, tt, pose);
        rig::evaluate(&rs.plan, pose, model);
        for step in &rs.plan {
            let i = step.node as usize;
            let (s, r, tr) = model[i].to_scale_rotation_translation();
            if let Ok(mut tf) = joints.get_mut(npc.joints[i]) {
                tf.translation = tr;
                tf.rotation = r;
                tf.scale = s;
            }
        }
    }
    let us = t0.elapsed().as_micros() as u32;
    let s = &mut n.stats;
    s.live = live;
    s.visible = visible;
    s.animated_last = animated;
    s.anim_us_last = us;
    s.anim_us_peak = s.anim_us_peak.max(us);
    s.skin_tris_visible = tris;
}

fn npc_report(time: Res<Time>, mut acc: Local<f32>, n: Res<Npcs>) {
    *acc += time.delta_secs();
    if *acc < 2.0 {
        return;
    }
    *acc = 0.0;
    let s = &n.stats;
    info!(
        "[npcs] live={} visible={} animated_last={} anim_us={} (peak {}) manage_us={} (peak {}) spawned_total={} assets: {} variants, {} attachments, {} jobs",
        s.live,
        s.visible,
        s.animated_last,
        s.anim_us_last,
        s.anim_us_peak,
        s.manage_us_last,
        s.manage_us_peak,
        s.spawned_total,
        n.variants.len(),
        n.attach.len(),
        n.workers.pending()
    );
}
