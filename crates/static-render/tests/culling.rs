//! Cluster culling agrees with brute force, never loses a visible instance, and is
//! deterministic.
use glam::{Affine3A, Mat4, Quat, Vec3};
use gothic_static_render::{Aabb, ClusterBvh, CullQuery, Frustum, cascade_caster_volume};

/// Deterministic pseudo-random scatter of `n` boxes over a 2 km square.
fn scene(n: usize) -> Vec<Aabb> {
    let mut s = 0x1234_5678_9abc_def0u64;
    let mut r = move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        (s >> 11) as f32 / (1u64 << 53) as f32
    };
    (0..n)
        .map(|_| {
            let local = Aabb::new(
                Vec3::new(-1.0, 0.0, -1.0),
                Vec3::new(1.0, 1.0 + r() * 6.0, 1.0),
            );
            let t = Affine3A::from_scale_rotation_translation(
                Vec3::splat(0.5 + r() * 2.0),
                Quat::from_rotation_y(r() * std::f32::consts::TAU),
                Vec3::new(r() * 2000.0 - 1000.0, 0.0, r() * 2000.0 - 1000.0),
            );
            local.transformed(&t)
        })
        .collect()
}

fn camera(eye: Vec3, target: Vec3) -> (Mat4, Frustum) {
    let view = glam::camera::rh::view::look_at_mat4(eye, target, Vec3::Y);
    let proj = glam::camera::rh::proj::directx::perspective_infinite_reverse(
        70f32.to_radians(),
        16.0 / 9.0,
        0.1,
    );
    (
        view.inverse(),
        Frustum::from_clip_from_world(&(proj * view), false),
    )
}

#[test]
fn clusters_cover_exactly_the_visible_instances() {
    let boxes = scene(10_000);
    let bvh = ClusterBvh::build(&boxes, 64);
    assert!(bvh.clusters.iter().all(|c| c.count as usize <= 64));
    assert_eq!(bvh.order.len(), boxes.len());
    let mut seen = vec![false; boxes.len()];
    for &i in &bvh.order {
        assert!(!seen[i as usize]);
        seen[i as usize] = true;
    }
    for (eye, target) in [
        (Vec3::new(0.0, 20.0, 0.0), Vec3::new(100.0, 0.0, 0.0)),
        (Vec3::new(-900.0, 50.0, -900.0), Vec3::new(0.0, 0.0, 0.0)),
        (Vec3::new(300.0, 300.0, 0.0), Vec3::new(300.0, 0.0, 1.0)),
    ] {
        let (_, f) = camera(eye, target);
        let q = CullQuery {
            frustum: &f,
            eye,
            max_distance: 600.0,
        };
        let mut vis = vec![false; bvh.clusters.len()];
        let stats = bvh.cull(&q, &mut vis);
        // Every instance visible by brute force sits in a visible cluster (conservative).
        let mut brute = 0;
        for (i, b) in boxes.iter().enumerate() {
            if f.intersects_aabb(b) && b.distance(eye) <= 600.0 {
                brute += 1;
                assert!(vis[bvh.cluster_of[i] as usize], "instance {i} lost");
            }
        }
        // And culling actually removes most of the scene.
        let kept: usize = vis
            .iter()
            .enumerate()
            .filter(|(_, v)| **v)
            .map(|(c, _)| bvh.clusters[c].count as usize)
            .sum();
        assert!(kept >= brute);
        assert!(kept < boxes.len() / 2, "kept {kept} of {}", boxes.len());
        assert!((stats.nodes_visited as usize) < bvh.clusters.len() * 2);
    }
}

#[test]
fn shadow_cascades_add_casters_outside_the_view() {
    let boxes = scene(10_000);
    let bvh = ClusterBvh::build(&boxes, 64);
    let eye = Vec3::new(0.0, 2.0, 0.0);
    let (world_from_view, f) = camera(eye, Vec3::new(0.0, 2.0, -100.0));
    let mut view = vec![false; bvh.clusters.len()];
    bvh.cull(
        &CullQuery {
            frustum: &f,
            eye,
            max_distance: 400.0,
        },
        &mut view,
    );
    let mut all = view.clone();
    // Low sun from the side: casters to the side of the view throw shadows into it.
    let light = Vec3::new(-1.0, -0.4, 0.0).normalize();
    let bounds = [0.1, 15.0, 50.0, 150.0];
    for w in bounds.windows(2) {
        let v = cascade_caster_volume(
            &world_from_view,
            70f32.to_radians(),
            16.0 / 9.0,
            w[0],
            w[1],
            light,
            200.0,
        );
        bvh.cull(
            &CullQuery {
                frustum: &v,
                eye,
                max_distance: f32::INFINITY,
            },
            &mut all,
        );
    }
    let n = |m: &[bool]| m.iter().filter(|v| **v).count();
    assert!(
        n(&all) > n(&view),
        "cascades add casters ({} vs {})",
        n(&all),
        n(&view)
    );
    assert!(
        view.iter().zip(&all).all(|(v, a)| !v || *a),
        "view clusters stay"
    );
    assert!(n(&all) < bvh.clusters.len() / 2);
}

#[test]
fn build_and_cull_are_deterministic() {
    let boxes = scene(3_000);
    let a = ClusterBvh::build(&boxes, 32);
    let b = ClusterBvh::build(&boxes, 32);
    assert_eq!(a.order, b.order);
    assert_eq!(a.clusters, b.clusters);
    assert!(ClusterBvh::build(&[], 32).is_empty());
}
