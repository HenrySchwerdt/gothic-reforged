//! Bevy plugin: static mesh registry with LOD ranges, instance spawning laid out for Bevy's
//! GPU-driven batching, and cluster culling against the view and the shadow cascades.
//!
//! How the pieces fit:
//! * Every instance of a registered mesh shares one `Mesh` and one material handle per LOD,
//!   so Bevy batches all of them into one multi-draw-indirect batch and culls them per
//!   instance on the GPU (GPU preprocessing; [`StaticCullCamera`] gets `NoCpuCulling` when
//!   `gpu_culling` is on, which skips Bevy's per-entity CPU frustum test).
//! * LOD levels are separate entities with `VisibilityRange` (dithered cross-fade).
//! * Instances are grouped into BVH clusters. Each frame the clusters are tested against the
//!   camera frustum + draw distance and against the caster volume of every shadow cascade of
//!   the [`StaticShadowLight`]; clusters nobody can see are hidden as a whole, which keeps them
//!   out of the CPU work for the view *and* the per-cascade shadow caster lists.
use crate::bvh::{ClusterBvh, CullQuery};
use crate::geom::{Aabb, Frustum, cascade_caster_volume};
use crate::lod::LodRanges;
use bevy::camera::visibility::{NoCpuCulling, VisibilityRange, VisibilitySystems};
use bevy::light::{CascadeShadowConfig, NotShadowCaster};
use bevy::prelude::*;
use bevy::transform::TransformSystems;
use serde::Serialize;
use std::time::Instant;

pub struct StaticRenderPlugin {
    /// Instances per cluster.
    pub cluster_size: usize,
    /// Add `NoCpuCulling` to [`StaticCullCamera`]s (GPU per-instance culling).
    pub gpu_culling: bool,
    /// Turn cluster culling off (for A/B benchmarks).
    pub cluster_culling: bool,
    /// How far behind a cascade (towards the light) casters are still considered (m).
    pub caster_reach: f32,
    /// A cluster that leaves the view (and every cascade) is hidden only after this many
    /// frames; one that comes back is shown at once. Avoids visibility churn (each flip makes
    /// Bevy re-bin the cluster's entities) when the camera turns back and forth.
    pub hide_after_frames: u16,
}

impl Default for StaticRenderPlugin {
    fn default() -> Self {
        Self {
            cluster_size: 64,
            gpu_culling: true,
            cluster_culling: true,
            caster_reach: 150.0,
            hide_after_frames: 30,
        }
    }
}

/// The camera clusters are culled for.
#[derive(Component)]
pub struct StaticCullCamera;

/// The directional light whose cascades decide which off-screen clusters still cast shadows.
#[derive(Component)]
pub struct StaticShadowLight;

/// Handle to a registered mesh.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StaticMeshId(pub u32);

/// A static mesh with its levels of detail.
#[derive(Clone, Debug)]
pub struct StaticMeshDesc {
    /// One mesh per level, finest first.
    pub lods: Vec<Handle<Mesh>>,
    /// End distance of each level (same length as `lods`).
    pub ranges: LodRanges,
    pub material: Handle<StandardMaterial>,
    /// Local bounds (covering all levels).
    pub bounds: Aabb,
    pub cast_shadows: bool,
}

/// Registry and instance list. Register meshes, then [`StaticMeshes::add`] instances; they are
/// spawned (and the clusters rebuilt) in the next `Update`.
#[derive(Resource, Default)]
pub struct StaticMeshes {
    meshes: Vec<StaticMeshDesc>,
    pending: Vec<(StaticMeshId, Transform)>,
    instances: Vec<Instance>,
}

struct Instance {
    bounds: Aabb,
    entities: Vec<Entity>,
}

impl StaticMeshes {
    pub fn register(&mut self, desc: StaticMeshDesc) -> StaticMeshId {
        assert_eq!(
            desc.lods.len(),
            desc.ranges.levels(),
            "one LOD range per LOD mesh"
        );
        self.meshes.push(desc);
        StaticMeshId(self.meshes.len() as u32 - 1)
    }

    pub fn add(&mut self, mesh: StaticMeshId, transform: Transform) {
        self.pending.push((mesh, transform));
    }

    pub fn instance_count(&self) -> usize {
        self.instances.len() + self.pending.len()
    }

    pub fn mesh(&self, id: StaticMeshId) -> &StaticMeshDesc {
        &self.meshes[id.0 as usize]
    }
}

/// Per-frame numbers (for HUDs and the benchmark).
#[derive(Resource, Default, Debug, Clone, Serialize)]
pub struct StaticRenderStats {
    pub instances: usize,
    pub entities: usize,
    pub clusters: usize,
    pub visible_clusters: usize,
    /// Clusters kept only because a shadow cascade needs them.
    pub shadow_only_clusters: usize,
    pub visible_instances: usize,
    pub bvh_nodes_visited: u32,
    pub cull_micros: u64,
}

#[derive(Resource, Default)]
struct Clusters {
    bvh: ClusterBvh,
    /// State applied to the entities (None: not applied yet).
    shown: Vec<Option<bool>>,
    /// Frames each cluster has been out of view and of every cascade.
    unseen: Vec<u16>,
}

#[derive(Resource)]
struct Config {
    cluster_size: usize,
    gpu_culling: bool,
    cluster_culling: bool,
    caster_reach: f32,
    hide_after_frames: u16,
}

impl Plugin for StaticRenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StaticMeshes>()
            .init_resource::<StaticRenderStats>()
            .init_resource::<Clusters>()
            .insert_resource(Config {
                cluster_size: self.cluster_size,
                gpu_culling: self.gpu_culling,
                cluster_culling: self.cluster_culling,
                caster_reach: self.caster_reach,
                hide_after_frames: self.hide_after_frames,
            })
            .add_systems(Update, (flush, gpu_culling_cameras))
            .add_systems(
                PostUpdate,
                cull.after(TransformSystems::Propagate)
                    .before(VisibilitySystems::VisibilityPropagate),
            );
    }
}

fn gpu_culling_cameras(
    mut commands: Commands,
    config: Res<Config>,
    cams: Query<Entity, (With<StaticCullCamera>, Without<NoCpuCulling>)>,
) {
    if config.gpu_culling {
        for e in &cams {
            commands.entity(e).insert(NoCpuCulling);
        }
    }
}

fn flush(
    mut commands: Commands,
    config: Res<Config>,
    mut statics: ResMut<StaticMeshes>,
    mut clusters: ResMut<Clusters>,
    mut stats: ResMut<StaticRenderStats>,
) {
    if statics.pending.is_empty() {
        return;
    }
    let pending = std::mem::take(&mut statics.pending);
    for (id, t) in pending {
        let desc = statics.meshes[id.0 as usize].clone();
        let bounds = desc.bounds.transformed(&t.compute_affine());
        let mut entities = Vec::with_capacity(desc.lods.len());
        for (level, mesh) in desc.lods.iter().enumerate() {
            let (start_margin, end_margin) = desc.ranges.visibility_range(level);
            let mut e = commands.spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(desc.material.clone()),
                t,
                VisibilityRange {
                    start_margin,
                    end_margin,
                    use_aabb: false,
                },
            ));
            if !desc.cast_shadows {
                e.insert(NotShadowCaster);
            }
            entities.push(e.id());
        }
        statics.instances.push(Instance { bounds, entities });
    }
    let bounds: Vec<Aabb> = statics.instances.iter().map(|i| i.bounds).collect();
    clusters.bvh = ClusterBvh::build(&bounds, config.cluster_size);
    clusters.shown = vec![None; clusters.bvh.clusters.len()];
    clusters.unseen = vec![0; clusters.bvh.clusters.len()];
    stats.instances = statics.instances.len();
    stats.entities = statics.instances.iter().map(|i| i.entities.len()).sum();
    stats.clusters = clusters.bvh.clusters.len();
}

#[allow(clippy::too_many_arguments)]
fn cull(
    config: Res<Config>,
    statics: Res<StaticMeshes>,
    mut clusters: ResMut<Clusters>,
    mut stats: ResMut<StaticRenderStats>,
    cams: Query<(&Camera, &GlobalTransform, &Projection), With<StaticCullCamera>>,
    lights: Query<(&GlobalTransform, Option<&CascadeShadowConfig>), With<StaticShadowLight>>,
    mut vis: Query<&mut Visibility>,
) {
    if clusters.bvh.is_empty() {
        return;
    }
    let start = Instant::now();
    let n = clusters.bvh.clusters.len();
    let mut view_mask = vec![!config.cluster_culling; n];
    let mut mask = view_mask.clone();
    let mut visited = 0;
    if config.cluster_culling {
        let Some((camera, cam_t, projection)) = cams.iter().next() else {
            return;
        };
        let world_from_view = cam_t.to_matrix();
        let clip_from_world = camera.clip_from_view() * world_from_view.inverse();
        let frustum = Frustum::from_clip_from_world(&clip_from_world, false);
        let eye = cam_t.translation();
        // Draw distance: the farthest LOD end of any registered mesh.
        let draw = statics
            .meshes
            .iter()
            .map(|m| m.ranges.draw_distance() + m.ranges.margin)
            .fold(0.0, f32::max);
        let s = clusters.bvh.cull(
            &CullQuery {
                frustum: &frustum,
                eye,
                max_distance: draw,
            },
            &mut view_mask,
        );
        visited += s.nodes_visited;
        mask.clone_from(&view_mask);
        if let (Some((light_t, Some(cfg))), Projection::Perspective(p)) =
            (lights.iter().next(), projection)
        {
            let light_dir = light_t.forward().as_vec3();
            let mut near = cfg.minimum_distance;
            for &far in &cfg.bounds {
                let volume = cascade_caster_volume(
                    &world_from_view,
                    p.fov,
                    p.aspect_ratio,
                    near.max(p.near),
                    far,
                    light_dir,
                    config.caster_reach,
                );
                let s = clusters.bvh.cull(
                    &CullQuery {
                        frustum: &volume,
                        eye,
                        max_distance: f32::INFINITY,
                    },
                    &mut mask,
                );
                visited += s.nodes_visited;
                near = far * (1.0 - cfg.overlap_proportion);
            }
        }
    }
    let mut visible_instances = 0;
    let mut shadow_only = 0;
    let clusters = &mut *clusters;
    for (c, &needed) in mask.iter().enumerate() {
        if needed {
            visible_instances += clusters.bvh.clusters[c].count as usize;
            shadow_only += usize::from(!view_mask[c]);
            clusters.unseen[c] = 0;
        } else {
            clusters.unseen[c] = clusters.unseen[c].saturating_add(1);
        }
        let on = needed || clusters.unseen[c] < config.hide_after_frames;
        if clusters.shown[c] == Some(on) {
            continue;
        }
        clusters.shown[c] = Some(on);
        let v = if on {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        for &i in clusters.bvh.members(c) {
            for &e in &statics.instances[i as usize].entities {
                if let Ok(mut vv) = vis.get_mut(e) {
                    *vv = v;
                }
            }
        }
    }
    stats.visible_clusters = mask.iter().filter(|m| **m).count();
    stats.shadow_only_clusters = shadow_only;
    stats.visible_instances = visible_instances;
    stats.bvh_nodes_visited = visited;
    stats.cull_micros = start.elapsed().as_micros() as u64;
}
