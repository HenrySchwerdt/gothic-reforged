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
use glam::Vec3;
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
        assert_eq!(
            self.world.raycast(origin, dir, max),
            self.brute_raycast(origin, dir, max),
            "distance mismatch for origin={origin:?}, dir={dir:?}, max={max}"
        );
        assert_eq!(
            self.world.raycast_hit(origin, dir, max),
            self.brute_raycast_hit(origin, dir, max),
            "hit/normal mismatch for origin={origin:?}, dir={dir:?}, max={max}"
        );
    }
}

fn triangle(a: Vec3, b: Vec3, c: Vec3) -> Tri {
    let tri = [a, b, c];
    assert!(tri.iter().all(|p| p.is_finite()));
    assert!((b - a).cross(c - a).length_squared() > 1e-6);
    tri
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
        let distance = rng.between(300.0, 2_000.0);
        hit_rays.push((center + normal * distance, -normal, distance + 10.0));
        fixture.insert(id, vec![tri]);
    }

    assert!(
        hit_rays.len() >= 80,
        "seeded fixture unexpectedly degenerate"
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
