//! Static props through `gothic-static-render` (`--static-render`): the world loader's static
//! parts (placed meshes and the merged small / mid / big batches) are grouped into the crate's
//! cluster BVH and whole clusters that neither the camera nor any shadow cascade can see are
//! hidden (with the crate's hysteresis). With the `gpu` part of the mode the camera gets
//! `NoCpuCulling`, so per-instance frustum culling runs in Bevy's GPU preprocessing pass.
//!
//! The props keep everything the walk gives them: their `GothicMaterial`, the shared mesh /
//! material handles (which is what lets Bevy batch them into multi-draw-indirect calls), the
//! per-entity `VisibilityRange` of the original small-object culling and LoD stages, and their
//! shadow flags. Cluster culling only ever hides what is out of the view frustum and out of
//! every cascade's caster volume, so the culling semantics stay those of the walk.
//!
//! The crate's own `StaticRenderPlugin` registry is not used: it spawns its own entities with a
//! `StandardMaterial` and one LoD table per mesh, and has no way to remove instances, while
//! the walk streams tiles in and out and computes the LoD ranges per placed instance.
use super::StaticMode;
use crate::world_loader::PartKind;
use bevy::camera::primitives::Aabb as BevyAabb;
use bevy::camera::visibility::{NoCpuCulling, VisibilitySystems};
use bevy::light::CascadeShadowConfig;
use bevy::prelude::*;
use bevy::transform::TransformSystems;
use gothic_static_render::{Aabb, ClusterBvh, CullQuery, Frustum, cascade_caster_volume};
use std::sync::OnceLock;
use std::time::Instant;

/// Instances per cluster (the crate's default).
const CLUSTER_SIZE: usize = 64;
/// A cluster out of view (and of every cascade) for this many frames is hidden.
const HIDE_AFTER_FRAMES: u16 = 30;
/// Clusters are rebuilt at most this often while tiles stream in or out.
const REBUILD_FRAMES: u32 = 30;
/// How far behind a cascade (towards the light) casters still count (metres).
const CASTER_REACH: f32 = 150.0;

static ENABLED: OnceLock<bool> = OnceLock::new();

/// Marks a world-loader part that goes through the cluster culler.
#[derive(Component)]
pub(crate) struct StaticProp;

/// The walk's hook: should this freshly spawned part get [`StaticProp`]?
pub(crate) fn routes(kind: PartKind) -> bool {
    ENABLED.get().copied().unwrap_or(false)
        && matches!(
            kind,
            PartKind::Static | PartKind::MergedSmall | PartKind::MergedMid | PartKind::MergedBig
        )
}

/// Cluster visibility with hysteresis (Bevy-free): a cluster is shown as soon as it is needed
/// and hidden only after `hide_after` frames without being needed.
#[derive(Default)]
pub(crate) struct ClusterState {
    pub bvh: ClusterBvh,
    shown: Vec<Option<bool>>,
    unseen: Vec<u16>,
}

impl ClusterState {
    pub fn new(bounds: &[Aabb], cluster_size: usize) -> Self {
        let bvh = ClusterBvh::build(bounds, cluster_size);
        let n = bvh.clusters.len();
        Self {
            bvh,
            shown: vec![None; n],
            unseen: vec![0; n],
        }
    }

    /// Feed one frame's needed-mask; returns the clusters whose state flips (cluster, shown).
    pub fn step(&mut self, needed: &[bool], hide_after: u16) -> Vec<(usize, bool)> {
        let mut flips = Vec::new();
        for (c, &need) in needed.iter().enumerate() {
            if need {
                self.unseen[c] = 0;
            } else {
                self.unseen[c] = self.unseen[c].saturating_add(1);
            }
            let on = need || self.unseen[c] < hide_after;
            if self.shown[c] != Some(on) {
                self.shown[c] = Some(on);
                flips.push((c, on));
            }
        }
        flips
    }
}

/// Numbers for the log (printed every 10 s with `GR_PROF`).
#[derive(Default, Debug, Clone)]
pub(crate) struct Stats {
    pub instances: usize,
    pub clusters: usize,
    pub visible_clusters: usize,
    pub shadow_only_clusters: usize,
    pub rebuilds: u32,
    pub cull_us_total: u64,
    pub frames: u64,
}

#[derive(Resource)]
struct StaticProps {
    mode: StaticMode,
    entities: Vec<Entity>,
    state: ClusterState,
    dirty: bool,
    since_rebuild: u32,
    stats: Stats,
    last_log: Option<Instant>,
}

pub(crate) fn install(app: &mut App, mode: StaticMode) {
    let _ = ENABLED.set(true);
    eprintln!(
        "static-render: cluster culling {}, GPU instance culling {}",
        if mode.cluster { "on" } else { "off" },
        if mode.gpu { "on" } else { "off" }
    );
    app.insert_resource(StaticProps {
        mode,
        entities: Vec::new(),
        state: ClusterState::default(),
        dirty: false,
        since_rebuild: REBUILD_FRAMES,
        stats: Stats::default(),
        last_log: None,
    })
    .add_systems(Update, gpu_culling_camera)
    .add_systems(
        PostUpdate,
        cull_props
            .after(TransformSystems::Propagate)
            .after(VisibilitySystems::CalculateBounds)
            .before(VisibilitySystems::VisibilityPropagate),
    );
}

fn gpu_culling_camera(
    mut commands: Commands,
    props: Res<StaticProps>,
    cams: Query<Entity, (With<crate::walk::PlayerCamera>, Without<NoCpuCulling>)>,
) {
    if props.mode.gpu {
        for e in &cams {
            commands.entity(e).insert(NoCpuCulling);
        }
    }
}

fn world_bounds(aabb: &BevyAabb, t: &GlobalTransform) -> Aabb {
    let local = Aabb::from_center_half(aabb.center.into(), aabb.half_extents.into());
    local.transformed(&t.affine())
}

#[allow(clippy::too_many_arguments)]
fn cull_props(
    mut props: ResMut<StaticProps>,
    added: Query<(), Added<StaticProp>>,
    mut removed: RemovedComponents<StaticProp>,
    all: Query<(Entity, &BevyAabb, &GlobalTransform), With<StaticProp>>,
    cams: Query<(&Camera, &GlobalTransform, &Projection), With<crate::walk::PlayerCamera>>,
    lights: Query<(
        &GlobalTransform,
        &DirectionalLight,
        Option<&CascadeShadowConfig>,
    )>,
    mut vis: Query<&mut Visibility, With<StaticProp>>,
) {
    let props = &mut *props;
    if !props.mode.cluster {
        return;
    }
    props.dirty |= !added.is_empty() || removed.read().next().is_some();
    props.since_rebuild = props.since_rebuild.saturating_add(1);
    if props.dirty && props.since_rebuild >= REBUILD_FRAMES {
        // Props that have no bounds yet stay as they are (visible) until the next rebuild.
        let (entities, bounds): (Vec<Entity>, Vec<Aabb>) =
            all.iter().map(|(e, a, t)| (e, world_bounds(a, t))).unzip();
        props.state = ClusterState::new(&bounds, CLUSTER_SIZE);
        props.entities = entities;
        props.dirty = false;
        props.since_rebuild = 0;
        props.stats.rebuilds += 1;
        props.stats.instances = props.entities.len();
        props.stats.clusters = props.state.bvh.clusters.len();
    }
    if props.state.bvh.is_empty() {
        return;
    }
    let Some((camera, cam_t, projection)) = cams.iter().next() else {
        return;
    };
    let start = Instant::now();
    let n = props.state.bvh.clusters.len();
    let world_from_view = cam_t.to_matrix();
    let clip_from_world = camera.clip_from_view() * world_from_view.inverse();
    let frustum = Frustum::from_clip_from_world(&clip_from_world, false);
    let eye = cam_t.translation();
    let far = match projection {
        Projection::Perspective(p) => p.far,
        _ => f32::INFINITY,
    };
    let mut view_mask = vec![false; n];
    props.state.bvh.cull(
        &CullQuery {
            frustum: &frustum,
            eye,
            max_distance: far,
        },
        &mut view_mask,
    );
    let mut mask = view_mask.clone();
    // The shadow-casting sun (other directional lights cast no cascades).
    let sun = lights
        .iter()
        .find(|(_, l, c)| l.shadow_maps_enabled && c.is_some());
    if let (Some((light_t, _, Some(cfg))), Projection::Perspective(p)) = (sun, projection) {
        let light_dir = light_t.forward().as_vec3();
        let mut near = cfg.minimum_distance;
        for &cascade_far in &cfg.bounds {
            let volume = cascade_caster_volume(
                &world_from_view,
                p.fov,
                p.aspect_ratio,
                near.max(p.near),
                cascade_far,
                light_dir,
                CASTER_REACH,
            );
            props.state.bvh.cull(
                &CullQuery {
                    frustum: &volume,
                    eye,
                    max_distance: f32::INFINITY,
                },
                &mut mask,
            );
            near = cascade_far * (1.0 - cfg.overlap_proportion);
        }
    }
    for (c, on) in props.state.step(&mask, HIDE_AFTER_FRAMES) {
        let v = if on {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        for &i in props.state.bvh.members(c) {
            if let Some(&e) = props.entities.get(i as usize)
                && let Ok(mut vv) = vis.get_mut(e)
            {
                *vv = v;
            }
        }
    }
    let s = &mut props.stats;
    s.visible_clusters = mask.iter().filter(|m| **m).count();
    s.shadow_only_clusters = mask
        .iter()
        .zip(&view_mask)
        .filter(|(m, v)| **m && !**v)
        .count();
    s.cull_us_total += start.elapsed().as_micros() as u64;
    s.frames += 1;
    if std::env::var_os("GR_PROF").is_some()
        && props.last_log.is_none_or(|t| t.elapsed().as_secs() >= 10)
    {
        props.last_log = Some(Instant::now());
        let s = &props.stats;
        eprintln!(
            "static-render: {} props in {} clusters, {} visible ({} shadow only), {} rebuilds, cull {:.1} us/frame",
            s.instances,
            s.clusters,
            s.visible_clusters,
            s.shadow_only_clusters,
            s.rebuilds,
            s.cull_us_total as f64 / s.frames.max(1) as f64
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::Vec3;

    fn grid(n: usize) -> Vec<Aabb> {
        (0..n)
            .map(|i| {
                let c = Vec3::new((i % 10) as f32 * 10.0, 0.0, (i / 10) as f32 * 10.0);
                Aabb::from_center_half(c, Vec3::splat(1.0))
            })
            .collect()
    }

    #[test]
    fn cloud_statics_hysteresis() {
        let mut s = ClusterState::new(&grid(100), 16);
        let n = s.bvh.clusters.len();
        assert!(n > 1);
        // First frame: everything gets a state (shown, since nothing was unseen long enough).
        let flips = s.step(&vec![false; n], 3);
        assert_eq!(flips.len(), n);
        assert!(flips.iter().all(|(_, on)| *on));
        // Hidden only after `hide_after` frames out of view.
        assert!(s.step(&vec![false; n], 3).is_empty());
        let flips = s.step(&vec![false; n], 3);
        assert_eq!(flips.len(), n);
        assert!(flips.iter().all(|(_, on)| !*on));
        // Needed again: shown at once.
        let mut need = vec![false; n];
        need[0] = true;
        assert_eq!(s.step(&need, 3), vec![(0, true)]);
    }

    #[test]
    fn cloud_statics_clusters_cover_every_prop() {
        let s = ClusterState::new(&grid(300), CLUSTER_SIZE);
        let mut seen: Vec<u32> = (0..s.bvh.clusters.len())
            .flat_map(|c| s.bvh.members(c).to_vec())
            .collect();
        seen.sort_unstable();
        assert_eq!(seen, (0..300).collect::<Vec<u32>>());
    }

    #[test]
    fn cloud_statics_routes_only_static_kinds_when_enabled() {
        // `ENABLED` is process-wide; the walk sets it once in `install`.
        let _ = ENABLED.set(true);
        assert!(routes(PartKind::Static));
        assert!(routes(PartKind::MergedBig));
        assert!(!routes(PartKind::Terrain));
        assert!(!routes(PartKind::Water));
        assert!(!routes(PartKind::Tree));
        assert!(!routes(PartKind::Foliage));
    }
}
