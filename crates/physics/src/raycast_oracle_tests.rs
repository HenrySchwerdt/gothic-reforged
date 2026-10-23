//! Brute-force oracle tests for the XZ-grid ray traversal.
//!
//! The oracle enumerates fixture triangles rather than calling `CollisionWorld::query`, so it
//! tests broad-phase coverage independently while sharing `ray_triangle` as the narrow phase.
//!
//! DDA implementation note: only early-out when best_t is strictly before the next cell
//! boundary. If best_t equals that boundary, process every supercover/boundary-adjacent cell
//! at that t first; otherwise an equal-distance hit (or raycast_hit normal tie winner) may be
//! skipped.

use super::{CollisionWorld, GRID, Tri, ray_triangle};
use glam::{Quat, Vec3};
use std::collections::BTreeMap;

#[derive(Default)]
struct FixtureWorld {
    world: CollisionWorld,
    fixtures: BTreeMap<u64, Vec<Tri>>,
}

impl FixtureWorld {
    fn insert(&mut self, id: u64, tris: Vec<Tri>) {
        assert!(
            tris.iter().all(|t| {
                t.iter().all(|p| p.is_finite())
                    && (t[1] - t[0]).cross(t[2] - t[0]).length_squared() > 1e-6
            }),
            "oracle fixtures must be finite and nondegenerate"
        );
        self.world.insert_chunk(id, tris.clone());
        self.fixtures.insert(id, tris);
    }

    fn remove(&mut self, id: u64) {
        self.world.remove_chunk(id);
        self.fixtures.remove(&id);
    }

    fn brute_raycast(&self, origin: Vec3, dir: Vec3, max: f32) -> Option<f32> {
        self.fixtures
            .iter()
            .flat_map(|(_, tris)| tris.iter())
            .filter_map(|tri| ray_triangle(origin, dir, tri))
            .filter(|&distance| distance <= max)
            .min_by(f32::total_cmp)
    }

    /// Mirrors raycast_hit's documented result and current deterministic tie behavior:
    /// sorted chunk id / triangle index, then replace only for a strictly nearer hit.
    fn brute_raycast_hit(&self, origin: Vec3, dir: Vec3, max: f32) -> Option<(f32, Vec3)> {
        let mut best: Option<(f32, Vec3)> = None;
        for tri in self.fixtures.values().flat_map(|tris| tris.iter()) {
            if let Some(distance) = ray_triangle(origin, dir, tri)
                && distance <= max
                && best.is_none_or(|previous| distance < previous.0)
            {
                let mut normal = (tri[1] - tri[0]).cross(tri[2] - tri[0]).normalize_or_zero();
                if normal.dot(dir) > 0.0 {
                    normal = -normal;
                }
                best = Some((distance, normal));
            }
        }
        best
    }

    fn assert_matches(&self, origin: Vec3, dir: Vec3, max: f32) {
        assert!(origin.is_finite() && dir.is_finite() && max.is_finite());
        assert!(
            (dir.length_squared() - 1.0).abs() < 2e-5,
            "ray must be normalized"
        );
        let expected = self.brute_raycast(origin, dir, max);
        let expected_hit = self.brute_raycast_hit(origin, dir, max);
        let expected_bits = expected.map(f32::to_bits);
        let expected_hit_bits = expected_hit.map(|(distance, normal)| (distance.to_bits(), normal));
        assert_eq!(
            self.world.raycast(origin, dir, max),
            expected,
            "distance mismatch for origin={origin:?}, dir={dir:?}, max={max}"
        );
        assert_eq!(
            self.world.raycast_accelerated(origin, dir, max),
            expected,
            "public accelerated distance mismatch for origin={origin:?}, dir={dir:?}, max={max}"
        );
        assert_eq!(
            self.world.raycast_hit(origin, dir, max),
            expected_hit,
            "hit/normal mismatch for origin={origin:?}, dir={dir:?}, max={max}"
        );
        assert_eq!(
            self.world
                .raycast_with_mode(origin, dir, max, true)
                .map(f32::to_bits),
            expected_bits,
            "DDA distance mismatch for origin={origin:?}, dir={dir:?}, max={max}"
        );
        assert_eq!(
            self.world
                .raycast_hit_with_mode(origin, dir, max, true)
                .map(|(distance, normal)| (distance.to_bits(), normal)),
            expected_hit_bits,
            "DDA hit/normal mismatch for origin={origin:?}, dir={dir:?}, max={max}"
        );
        assert_eq!(
            self.world
                .raycast_with_mode(origin, dir, max, false)
                .map(f32::to_bits),
            expected_bits,
            "legacy distance mismatch for origin={origin:?}, dir={dir:?}, max={max}"
        );
        assert_eq!(
            self.world
                .raycast_hit_with_mode(origin, dir, max, false)
                .map(|(distance, normal)| (distance.to_bits(), normal)),
            expected_hit_bits,
            "legacy hit/normal mismatch for origin={origin:?}, dir={dir:?}, max={max}"
        );
    }
}

fn triangle(a: Vec3, b: Vec3, c: Vec3) -> Tri {
    let tri = [a, b, c];
    assert!(tri.iter().all(|p| p.is_finite()));
    assert!((b - a).cross(c - a).length_squared() > 1e-6);
    tri
}

fn ray_target_triangle(center: Vec3, dir: Vec3) -> Tri {
    let right = dir.cross(Vec3::Y).normalize();
    let up = dir.cross(right).normalize();
    triangle(
        center - right * 100.0 - up * 100.0,
        center + right * 100.0 - up * 100.0,
        center + up * 200.0,
    )
}

fn ray_rectangle_cell_count(origin: Vec3, end: Vec3) -> u64 {
    let lo = origin.min(end) - 1.0;
    let hi = origin.max(end) + 1.0;
    (super::cell(hi.x).abs_diff(super::cell(lo.x)) as u64 + 1)
        .saturating_mul(super::cell(hi.z).abs_diff(super::cell(lo.z)) as u64 + 1)
}

fn quad_xz(y: f32, cx: f32, cz: f32, half: f32) -> Vec<Tri> {
    let a = Vec3::new(cx - half, y, cz - half);
    let b = Vec3::new(cx + half, y, cz - half);
    let c = Vec3::new(cx + half, y, cz + half);
    let d = Vec3::new(cx - half, y, cz + half);
    vec![triangle(a, c, b), triangle(a, d, c)]
}

fn quad_yz(x: f32, cy: f32, cz: f32, half: f32) -> Vec<Tri> {
    let a = Vec3::new(x, cy - half, cz - half);
    let b = Vec3::new(x, cy + half, cz - half);
    let c = Vec3::new(x, cy + half, cz + half);
    let d = Vec3::new(x, cy - half, cz + half);
    vec![triangle(a, b, c), triangle(a, c, d)]
}

fn quad_xy(z: f32, cx: f32, cy: f32, half: f32) -> Vec<Tri> {
    let a = Vec3::new(cx - half, cy - half, z);
    let b = Vec3::new(cx + half, cy - half, z);
    let c = Vec3::new(cx + half, cy + half, z);
    let d = Vec3::new(cx - half, cy + half, z);
    vec![triangle(a, b, c), triangle(a, c, d)]
}

#[derive(Clone, Copy)]
struct XorShift64(u64);

impl XorShift64 {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn unit(&mut self) -> f32 {
        ((self.next() >> 40) as u32) as f32 / 16_777_216.0
    }

    fn between(&mut self, lo: f32, hi: f32) -> f32 {
        lo + self.unit() * (hi - lo)
    }

    fn vec3(&mut self, lo: f32, hi: f32) -> Vec3 {
        Vec3::new(
            self.between(lo, hi),
            self.between(lo, hi),
            self.between(lo, hi),
        )
    }

    fn unit_vec3(&mut self) -> Vec3 {
        loop {
            let v = self.vec3(-1.0, 1.0);
            if v.length_squared() > 0.05 {
                return v.normalize();
            }
        }
    }
}

#[test]
fn grid_raycast_matches_fixture_oracle_for_seeded_hits_and_misses() {
    let mut rng = XorShift64(0x4d59_5df4_d0f3_3173);
    let mut fixture = FixtureWorld::default();
    let mut hit_rays = Vec::new();
    let mut dda_hit_rays = 0;

    for id in 1..=96 {
        let a = rng.vec3(-8_000.0, 8_000.0);
        let e1 = rng.vec3(-350.0, 350.0);
        let e2 = rng.vec3(-350.0, 350.0);
        let cross = e1.cross(e2);
        if cross.length_squared() <= 1e-3 {
            continue;
        }
        let tri = triangle(a, a + e1, a + e2);
        let center = (tri[0] + tri[1] + tri[2]) / 3.0;
        let normal = cross.normalize();
        let distance = rng.between(50_000.0, 90_000.0);
        if normal.x.abs() > 0.4 && normal.z.abs() > 0.4 {
            let end = center - normal * 10.0;
            let origin = center + normal * distance;
            assert!(
                super::prefer_ray_dda(origin, end, origin.min(end) - 1.0, origin.max(end) + 1.0),
                "long seeded hit ray should select DDA"
            );
            dda_hit_rays += 1;
        }
        hit_rays.push((center + normal * distance, -normal, distance + 10.0));
        fixture.insert(id, vec![tri]);
    }

    assert!(
        hit_rays.len() >= 80,
        "seeded fixture unexpectedly degenerate"
    );
    assert!(
        dda_hit_rays >= 8,
        "expected multiple seeded hits to select DDA"
    );
    for (origin, dir, max) in hit_rays {
        assert!(fixture.brute_raycast(origin, dir, max).is_some());
        fixture.assert_matches(origin, dir, max);
    }

    for _ in 0..160 {
        let origin = rng.vec3(-10_000.0, 10_000.0);
        let dir = rng.unit_vec3();
        let max = rng.between(0.0, 10_000.0);
        fixture.assert_matches(origin, dir, max);
    }
}

#[test]
fn vertical_rays_cover_positive_negative_gridlines_corners_and_inclusive_max() {
    let mut fixture = FixtureWorld::default();
    let coordinates = [-2.0 * GRID, -GRID, 0.0, GRID, 2.0 * GRID];
    let mut id = 1;
    for &x in &coordinates {
        for &z in &coordinates {
            fixture.insert(id, quad_xz(0.0, x, z, 32.0));
            id += 1;
        }
    }

    for &x in &coordinates {
        for &z in &coordinates {
            for dx in [-0.01, 0.0, 0.01] {
                for dz in [-0.01, 0.0, 0.01] {
                    // Exact inclusive endpoint: hit distance equals max.
                    fixture.assert_matches(Vec3::new(x + dx, 100.0, z + dz), Vec3::NEG_Y, 100.0);
                    assert_eq!(
                        fixture
                            .world
                            .raycast(Vec3::new(x + dx, 100.0, z + dz), Vec3::NEG_Y, 100.0,),
                        Some(100.0)
                    );
                }
            }
        }
    }
}

#[test]
fn axis_parallel_rays_visit_both_sides_of_parallel_gridlines() {
    let mut fixture = FixtureWorld::default();
    fixture.insert(1, quad_yz(0.0, 0.0, GRID, 32.0));
    fixture.insert(2, quad_xy(0.0, -GRID, 0.0, 32.0));

    // X ray lies on z=GRID; Z ray lies on x=-GRID.
    fixture.assert_matches(Vec3::new(-100.0, 0.0, GRID), Vec3::X, 100.0);
    fixture.assert_matches(Vec3::new(-GRID, 0.0, -100.0), Vec3::Z, 100.0);
    assert_eq!(
        fixture
            .world
            .raycast(Vec3::new(-100.0, 0.0, GRID), Vec3::X, 100.0),
        Some(100.0)
    );
    assert_eq!(
        fixture
            .world
            .raycast(Vec3::new(-GRID, 0.0, -100.0), Vec3::Z, 100.0),
        Some(100.0)
    );
}

#[test]
fn diagonal_corner_crossing_and_large_spanning_triangle_are_not_missed() {
    let mut fixture = FixtureWorld::default();
    fixture.insert(1, quad_xz(0.0, GRID, GRID, 32.0));
    let origin = Vec3::new(GRID - 100.0, 100.0, GRID - 100.0);
    let dir = Vec3::new(1.0, -1.0, 1.0).normalize();
    fixture.assert_matches(origin, dir, 300.0);

    let half = 31.0 * GRID; // Width is 62 cells, below insert_chunk's 64-cell guard.
    fixture.insert(
        2,
        vec![triangle(
            Vec3::new(-half, 0.0, -half),
            Vec3::new(half, 0.0, -half),
            Vec3::new(0.0, 0.0, half),
        )],
    );
    let center = Vec3::new(0.0, 0.0, -half / 3.0);
    fixture.assert_matches(center + Vec3::Y * 100.0, Vec3::NEG_Y, 100.0);
}

#[test]
fn long_diagonal_dda_matches_legacy_and_oracle_at_grid_corner_hit() {
    let mut fixture = FixtureWorld::default();
    fixture.insert(1, quad_xz(0.0, 0.0, 0.0, 80.0));
    // The ray reaches the exact XZ grid corner at (0, 0), exercising the halo on negative cells.
    let origin = Vec3::new(-50_000.0, 20_000.0, -50_000.0);
    let dir = Vec3::new(1.0, -0.4, 1.0).normalize();
    let max = 100_000.0;
    let end = origin + dir * max;
    let lo = origin.min(end) - 1.0;
    let hi = origin.max(end) + 1.0;
    assert!(super::prefer_ray_dda(origin, end, lo, hi));
    let cells = super::ray_dda_cells(origin, end).expect("bounded diagonal traversal");
    for x in [-1, 0] {
        for z in [-1, 0] {
            assert!(cells.contains(&(x, z)), "corner-adjacent cell ({x}, {z})");
        }
    }
    fixture.assert_matches(origin, dir, max);
    assert!(
        fixture
            .world
            .raycast_with_mode(origin, dir, max, true)
            .is_some()
    );
}

#[test]
fn default_pick_range_diagonal_matches_legacy_and_oracle() {
    let origin = Vec3::new(-5_000.0, 1_000.0, -5_000.0);
    let dir = Vec3::new(1.0, -0.1, 1.0).normalize();
    let max = 10_000.0;
    let end = origin + dir * max;
    let area = ray_rectangle_cell_count(origin, end);
    let cells = super::ray_dda_cells(origin, end).expect("bounded default-range traversal");
    assert!(super::prefer_ray_dda(
        origin,
        end,
        origin.min(end) - 1.0,
        origin.max(end) + 1.0
    ));
    assert!(
        (cells.len() as u64) < area,
        "DDA cells {}/rectangle {area}",
        cells.len()
    );

    let mut fixture = FixtureWorld::default();
    fixture.insert(1, vec![ray_target_triangle(origin + dir * 5_000.0, dir)]);
    assert!(fixture.brute_raycast(origin, dir, max).is_some());
    fixture.assert_matches(origin, dir, max);
}

#[test]
fn actual_aim_camera_pick_range_uses_dda_and_matches_oracle() {
    let rotation = Quat::from_xyzw(-0.15314464, 0.83887678, 0.30089217, 0.42696187);
    let origin = Vec3::new(45.200893, 53.970318, 7.260711) * 100.0;
    let dir = rotation * Vec3::NEG_Z;
    let max = 10_000.0;
    let end = origin + dir * max;
    let area = ray_rectangle_cell_count(origin, end);
    let cells = super::ray_dda_cells(origin, end).expect("bounded camera pick traversal");
    assert!(super::prefer_ray_dda(
        origin,
        end,
        origin.min(end) - 1.0,
        origin.max(end) + 1.0
    ));
    assert!(
        (cells.len() as u64) < area,
        "DDA cells {}/rectangle {area}",
        cells.len()
    );

    let mut fixture = FixtureWorld::default();
    fixture.insert(1, vec![ray_target_triangle(origin + dir * 5_000.0, dir)]);
    assert!(fixture.brute_raycast(origin, dir, max).is_some());
    fixture.assert_matches(origin, dir, max);
}

#[test]
fn dda_halo_covers_negative_and_positive_gridline_endpoints() {
    for (origin, end, expected) in [
        (
            Vec3::new(399.0, 0.0, 399.0),
            Vec3::new(801.0, 0.0, 801.0),
            vec![(0, 0), (1, 1), (2, 2)],
        ),
        (
            Vec3::new(-399.0, 0.0, -399.0),
            Vec3::new(-801.0, 0.0, -801.0),
            vec![(-1, -1), (-2, -2), (-3, -3)],
        ),
    ] {
        let cells = super::ray_dda_cells(origin, end).expect("bounded diagonal traversal");
        for (x, z) in expected {
            for ox in -1..=1 {
                for oz in -1..=1 {
                    assert!(cells.contains(&(x + ox, z + oz)));
                }
            }
        }
    }
}

#[test]
fn dda_is_not_selected_for_axis_parallel_small_or_extreme_segments() {
    for (origin, end) in [
        (Vec3::ZERO, Vec3::new(0.0, 100.0, 10_000.0)),
        (Vec3::ZERO, Vec3::new(10_000.0, 100.0, 0.0)),
        (Vec3::ZERO, Vec3::new(50_000.0, 0.0, 1.0)),
        (Vec3::ZERO, Vec3::new(800.0, 100.0, 800.0)),
        (
            Vec3::new(-20_000_000.0, 100.0, -20_000_000.0),
            Vec3::new(20_000_000.0, -100.0, 20_000_000.0),
        ),
    ] {
        let lo = origin.min(end) - 1.0;
        let hi = origin.max(end) + 1.0;
        assert!(!super::prefer_ray_dda(origin, end, lo, hi));
    }
    assert!(super::ray_dda_cells(Vec3::ZERO, Vec3::new(0.0, 0.0, 10_000.0)).is_none());
    assert!(super::ray_dda_cells(Vec3::ZERO, Vec3::new(-0.0, 0.0, 10_000.0)).is_none());
    assert!(super::ray_dda_cells(Vec3::ZERO, Vec3::new(10_000.0, 0.0, -0.0)).is_none());
    assert!(
        super::ray_dda_cells(Vec3::ZERO, Vec3::new(7_000_000.0, 0.0, 7_000_000.0)).is_none(),
        "step cap must bound DDA work"
    );
    assert!(
        super::ray_dda_cells(
            Vec3::new(-20_000_000.0, 0.0, -20_000_000.0),
            Vec3::new(20_000_000.0, 0.0, 20_000_000.0)
        )
        .is_none(),
        "extreme coordinates must use legacy fallback"
    );
}

#[test]
fn equal_distance_hit_keeps_lowest_chunk_triangle_order_for_normal() {
    let mut fixture = FixtureWorld::default();
    let z_equals_x = triangle(
        Vec3::new(0.0, -10.0, 0.0),
        Vec3::new(10.0, 10.0, 10.0),
        Vec3::new(-10.0, 0.0, -10.0),
    );
    let y_equals_x = triangle(
        Vec3::new(0.0, 0.0, -10.0),
        Vec3::new(10.0, 10.0, 10.0),
        Vec3::new(-10.0, -10.0, 0.0),
    );
    // Insert higher id first; grid insertion order must not alter current query's tie winner.
    fixture.insert(2, vec![y_equals_x]);
    fixture.insert(1, vec![z_equals_x]);

    let origin = Vec3::new(-100.0, 0.0, 0.0);
    let dir = Vec3::X;
    let d1 = ray_triangle(origin, dir, &z_equals_x).expect("first fixture hit");
    let d2 = ray_triangle(origin, dir, &y_equals_x).expect("second fixture hit");
    assert_eq!(d1, d2, "fixture must exercise a true distance tie");
    let expected = fixture.brute_raycast_hit(origin, dir, 100.0);
    let actual = fixture.world.raycast_hit(origin, dir, 100.0);
    assert_eq!(actual, expected);
    assert_eq!(actual.map(|hit| hit.0), Some(100.0));
    let expected_normal = (z_equals_x[1] - z_equals_x[0])
        .cross(z_equals_x[2] - z_equals_x[0])
        .normalize_or_zero();
    assert_eq!(actual.map(|hit| hit.1), Some(expected_normal));
}

#[test]
fn equal_distance_long_dda_keeps_legacy_triangle_order_and_normal() {
    let mut fixture = FixtureWorld::default();
    let x_plane = triangle(
        Vec3::new(0.0, -100.0, -100.0),
        Vec3::new(0.0, -100.0, 100.0),
        Vec3::new(0.0, 200.0, 0.0),
    );
    let z_plane = triangle(
        Vec3::new(-100.0, -100.0, 0.0),
        Vec3::new(100.0, -100.0, 0.0),
        Vec3::new(0.0, 200.0, 0.0),
    );
    fixture.insert(2, vec![z_plane]);
    fixture.insert(1, vec![x_plane]);

    let origin = Vec3::new(-50_000.0, 0.0, -50_000.0);
    let dir = Vec3::new(1.0, 0.0, 1.0).normalize();
    let max = 100_000.0;
    let end = origin + dir * max;
    assert!(super::prefer_ray_dda(
        origin,
        end,
        origin.min(end) - 1.0,
        origin.max(end) + 1.0
    ));
    let x_hit = ray_triangle(origin, dir, &x_plane).expect("x plane ray hit");
    let z_hit = ray_triangle(origin, dir, &z_plane).expect("z plane ray hit");
    assert_eq!(x_hit, z_hit, "fixture must be an exact distance tie");
    fixture.assert_matches(origin, dir, max);
    let hit = fixture
        .world
        .raycast_hit_with_mode(origin, dir, max, true)
        .unwrap();
    assert_eq!(hit.0, x_hit);
    assert_eq!(
        hit.1,
        Vec3::NEG_X,
        "lowest chunk id must keep its oriented normal"
    );
}

#[test]
fn chunk_removal_and_reinsertion_match_the_fixture_oracle() {
    let mut fixture = FixtureWorld::default();
    let tris = quad_xz(0.0, -GRID, GRID, 25.0);
    fixture.insert(77, tris.clone());
    let origin = Vec3::new(-GRID, 400.0, GRID);
    let dir = Vec3::NEG_Y;
    fixture.assert_matches(origin, dir, 400.0);
    assert_eq!(fixture.world.raycast(origin, dir, 400.0), Some(400.0));

    fixture.remove(77);
    fixture.assert_matches(origin, dir, 400.0);
    assert_eq!(fixture.world.raycast(origin, dir, 400.0), None);

    fixture.insert(77, tris);
    fixture.assert_matches(origin, dir, 400.0);
    assert_eq!(fixture.world.raycast(origin, dir, 400.0), Some(400.0));
}
