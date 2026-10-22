//! Original render *rules* (not the D3D9 renderer): level-of-detail selection, small-object and
//! range fades, the per-object static light packet, vegetation fade and shadow constants.
//!
//! Bevy-free plain functions so they can be unit tested and used by any renderer. Sources:
//! `eCFrustumElementItem::UpdateNodeData` (Engine.dll:0x101a3390), `CalculateAreaFactor`
//! (0x101ac5a0), `CalculateLoDArea` (0x101a2f00), `eCRenderContextMesh::CalculateLoDStage`
//! (0x101d9080), `eCFrustumFilter::CullSmallObjects` (0x101ac6d0),
//! `eCIlluminationAdmin::GetDynamicStaticLightData` (0x100e5620), `eCVegetation_PS` handlers
//! (0x1027e8a0 / 0x1027eda0) and `eCDirectionalLight_PS::UpdateShadowMaps` (0x100e8630).

pub mod water;

pub mod lod {
    //! Level-of-detail rules.

    /// Port of `eCFrustumFilter::CalculateAreaFactor` (0x101ac5a0): the projected area of the
    /// bounding sphere relative to the screen, `pi * (r / z)^2` clamped to `[0, 1]`, with `z` the
    /// depth along the camera forward axis (not field-of-view dependent).
    pub fn area_factor(radius: f32, depth: f32) -> f32 {
        gothic_portcov::hit!("area_factor");
        let z = if depth == 0.0 { 1e-5 } else { depth };
        (std::f32::consts::PI * (radius / z).powi(2)).clamp(0.0, 1.0)
    }

    /// Port of `eCFrustumDatabase::CalculateLoDArea` (0x101a2f00): LoD input is the database's
    /// global LoD factor x area factor x entity LoD scale x entity-type LoD scale.
    pub fn lod_input(global: f32, area: f32, entity_scale: f32, type_scale: f32) -> f32 {
        gothic_portcov::hit!("lod_input");
        global * area * entity_scale * type_scale
    }

    /// Upper bounds of the four mesh stage bands (floats at Engine.dll:0x104263d8).
    pub const UPPER: [f32; 4] = [0.25, 0.04125, 0.0078125, 0.003407];
    /// Lower bounds (0x104263dc). Below the last one the coarsest stage is used.
    pub const LOWER: [f32; 4] = [0.04125, 0.0078125, 0.003407, 0.00095];
    pub const COARSEST_BELOW: f32 = 0.00095;

    /// Port of `eCRenderContextMesh::CalculateLoDStage` (0x101d9080). `stages` is the number of
    /// LoD meshes the visual has (0 or 1 means always stage 0).
    pub fn lod_stage(input: f32, stages: usize) -> usize {
        gothic_portcov::hit!("lod_stage");
        if stages <= 1 || input > UPPER[0] {
            return 0;
        }
        if input < COARSEST_BELOW {
            return stages - 1;
        }
        for i in 0..4 {
            if UPPER[i] >= input && input > LOWER[i] {
                return i.min(stages - 1);
            }
        }
        stages - 1
    }

    /// Camera depths (same unit as `radius`) at which the mesh stage changes, for a visual with
    /// `levels` LoD meshes: entry `k` is the depth where [`lod_stage`] switches from level `k`
    /// to `k + 1`. Inverts `lod_input = global * area_factor * entity_scale` with
    /// `area_factor = pi (r / z)^2` (the factor clamps at 1, i.e. no switch closer than
    /// `r sqrt(pi)`). At most four boundaries exist (the table has four lower bounds).
    pub fn stage_distances(radius: f32, levels: usize, global: f32, entity_scale: f32) -> Vec<f32> {
        let k = (global * entity_scale).max(1e-6);
        (0..levels.saturating_sub(1).min(LOWER.len()))
            .map(|i| {
                let area = (LOWER[i] / k).clamp(1e-9, 1.0);
                radius * (std::f32::consts::PI / area).sqrt()
            })
            .collect()
    }

    /// `fGlobalVisualLoDFactor` of the quality presets of `ge3.INI` (0 VeryHigh .. 3 Low) [I].
    pub fn global_factor(quality: u8) -> f32 {
        if quality == 0 { 1.0 } else { 0.01 }
    }
}

pub mod cull {
    //! Small-object culling and fade-out.

    /// `0.5 * (1 + sin(pi * (x - 0.5)))`: the ease of the fade (function at 0x10039530).
    pub fn ease(x: f32) -> f32 {
        0.5 * (1.0 + (std::f32::consts::PI * (x - 0.5)).sin())
    }

    /// Fraction of the threshold below which an object is culled completely (double 0.4).
    pub const BAND: f32 = 0.4;

    /// Port of the non-ranged branch of `eCFrustumFilter::CullSmallObjects` (0x101ac6d0).
    /// `threshold` = entity scale x database screen-cull factor x entity-type scale. Returns the
    /// fade-out value in `[0, 1]`; anything below 1 means the object is drawn in the alpha list.
    pub fn small_object_fade(area: f32, threshold: f32) -> f32 {
        gothic_portcov::hit!("small_object_fade");
        if area <= 0.0 {
            return 0.0;
        }
        let low = threshold - BAND * threshold;
        if area < low {
            return 0.0;
        }
        if area > threshold {
            return 1.0;
        }
        ease(1.0 - (threshold - area) / (BAND * threshold))
    }

    /// Ranged branch (flag 0x8000000): 0 beyond `range`, linear ramp over `window` just inside,
    /// 1 otherwise. `window` is 0.15 x range for the entity category and a settings value for the
    /// other.
    pub fn ranged_fade(distance: f32, range: f32, window: f32) -> f32 {
        if distance > range {
            return 0.0;
        }
        if window > 0.0 && distance > range - window {
            return ease((range - distance) / window);
        }
        1.0
    }

    /// Camera distances (start of the fade, fully culled) at which an object of bounding radius
    /// `radius` crosses the small-object band: the area factor is `pi (r / z)^2`, so the band
    /// `[0.6 S, S]` maps to `z = r sqrt(pi / S)` and `r sqrt(pi / (0.6 S))`.
    pub fn distance_band(radius: f32, threshold: f32) -> (f32, f32) {
        let z = |a: f32| radius * (std::f32::consts::PI / a).sqrt();
        (z(threshold), z((1.0 - BAND) * threshold))
    }

    /// Fade window of ranged entities.
    pub const RANGED_WINDOW: f32 = 0.15;

    /// `fScreenObjectDistanceCulling` of the presets (0 VeryHigh .. 3 Low) [I].
    pub fn screen_cull_factor(quality: u8) -> f32 {
        [0.002, 0.008, 0.02, 0.045][quality.min(3) as usize]
    }
}

pub mod lights {
    //! Per-object static point light packet.

    /// Maximum lights per object (`GetDynamicStaticLightData` trims the candidates to three).
    pub const MAX_STATIC: usize = 3;
    /// Candidate buffer length before trimming.
    pub const MAX_CANDIDATES: usize = 10;
    /// Colour multiplier written into the packet (double at 0x104885f0).
    pub const COLOR_BOOST: f32 = 2.0;

    /// A registered static light (property-set type 0x48).
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct StaticLight {
        pub position: [f32; 3],
        pub color: [f32; 3],
        pub range: f32,
    }

    /// The 7 x float4 structure-of-arrays packet the shader reads.
    #[derive(Debug, Clone, Copy, Default, PartialEq)]
    pub struct Packet {
        pub x: [f32; 4],
        pub y: [f32; 4],
        pub z: [f32; 4],
        pub r: [f32; 4],
        pub g: [f32; 4],
        pub b: [f32; 4],
        pub inv_range2: [f32; 4],
        pub count: usize,
    }

    /// Port of `eCStaticPointLight_PS::GetWight(bCSphere)` (Engine.dll:0x100f4120): the luminance
    /// of the light colour (`0.3 r + 0.59 g + 0.11 b`) times `1 - (d / range)^2`, with `d` the
    /// distance from the light to the surface of the object's bounding sphere (0 inside); lights
    /// beyond their range weigh 0.
    pub fn weight_sphere(light: &StaticLight, centre: [f32; 3], radius: f32) -> f32 {
        gothic_portcov::hit!("weight_sphere");
        let d2: f32 = (0..3)
            .map(|i| (light.position[i] - centre[i]).powi(2))
            .sum();
        let d = (d2.sqrt() - radius).max(0.0);
        let falloff = 1.0 - (d / light.range).powi(2);
        if falloff < 0.0 {
            return 0.0;
        }
        let lum = 0.3 * light.color[0] + 0.59 * light.color[1] + 0.11 * light.color[2];
        lum * falloff
    }

    /// Point variant (`GetWight(bCVector)`, 0x100f41f0): a sphere of radius 0.
    pub fn weight(light: &StaticLight, centre: [f32; 3]) -> f32 {
        weight_sphere(light, centre, 0.0)
    }

    /// Port of `eCStaticPointLight_PS::GetIntensity` (0x100f42e0): with `t = d / range` the
    /// smooth fall-off `2 (1 - 3 t^2 + 2 t^3)` times the cosine of the incidence angle
    /// (`param_2 = d cos`). 0 beyond the range.
    pub fn intensity(distance: f32, range: f32, cosine: f32) -> f32 {
        gothic_portcov::hit!("intensity");
        if distance > range || distance <= 0.0 {
            return 0.0;
        }
        let t = distance / range;
        2.0 * (1.0 - 3.0 * t * t + 2.0 * t * t * t) * cosine
    }

    /// Port of `eCIlluminationAdmin::GetDynamicStaticLightData` (0x100e5620): collect lights whose
    /// range box touches the object's box (at most 10 candidates), rank by weight (descending),
    /// keep three, write position, colour x 2 and `1 / range^2`.
    pub fn packet(
        box_min: [f32; 3],
        box_max: [f32; 3],
        lights: impl IntoIterator<Item = StaticLight>,
    ) -> Packet {
        gothic_portcov::hit!("packet");
        let centre: [f32; 3] = std::array::from_fn(|i| 0.5 * (box_min[i] + box_max[i]));
        let radius = 0.5
            * (0..3)
                .map(|i| (box_max[i] - box_min[i]).powi(2))
                .sum::<f32>()
                .sqrt();
        let mut cand: Vec<(f32, StaticLight)> = Vec::new();
        for l in lights {
            if cand.len() >= MAX_CANDIDATES {
                break;
            }
            let touches = (0..3).all(|i| {
                l.position[i] + l.range >= box_min[i] && l.position[i] - l.range <= box_max[i]
            });
            if touches && l.range > 0.0 {
                cand.push((weight_sphere(&l, centre, radius), l));
            }
        }
        cand.sort_by(|a, b| b.0.total_cmp(&a.0));
        cand.truncate(MAX_STATIC);
        let mut p = Packet {
            count: cand.len(),
            ..Default::default()
        };
        for (i, (_, l)) in cand.iter().enumerate() {
            p.x[i] = l.position[0];
            p.y[i] = l.position[1];
            p.z[i] = l.position[2];
            p.r[i] = l.color[0] * COLOR_BOOST;
            p.g[i] = l.color[1] * COLOR_BOOST;
            p.b[i] = l.color[2] * COLOR_BOOST;
            p.inv_range2[i] = 1.0 / (l.range * l.range);
        }
        p
    }

    /// Nearest-N variant used for a whole view: the `max` strongest lights whose range reaches
    /// `centre` + `radius` (the renderer feeds characters near the camera from one shared set).
    pub fn select(
        centre: [f32; 3],
        radius: f32,
        lights: impl IntoIterator<Item = StaticLight>,
        max: usize,
    ) -> Vec<StaticLight> {
        let mut cand: Vec<(f32, StaticLight)> = lights
            .into_iter()
            .filter(|l| {
                let d2: f32 = (0..3).map(|i| (l.position[i] - centre[i]).powi(2)).sum();
                l.range > 0.0 && d2.sqrt() <= l.range + radius
            })
            .map(|l| (weight(&l, centre), l))
            .collect();
        cand.sort_by(|a, b| b.0.total_cmp(&a.0));
        cand.truncate(max);
        cand.into_iter().map(|(_, l)| l).collect()
    }

    /// Light contribution at squared distance `d2` from a packet entry: the vertex shader of
    /// `ge_default.fx` (instance static lights) uses `max(1 - d2 / range^2, 0) * max(N.L, 0)`.
    pub fn attenuation(d2: f32, inv_range2: f32) -> f32 {
        (1.0 - d2 * inv_range2).clamp(0.0, 1.0)
    }
}

pub mod vegetation {
    //! Vegetation grid view and fade rules.

    /// `VegetationViewRange.fViewDistance_*` of the presets (cm), 0 VeryHigh .. 3 Low.
    pub fn view_range(quality: u8) -> f32 {
        [7500.0, 6500.0, 5000.0, 4000.0][quality.min(3) as usize]
    }

    /// Fade start as a fraction of the view range: the original draws a fresh random factor in
    /// `[0.65, 0.75]` (`eCVegetation_PS::OnRender`, 0x1027e8a0).
    pub fn fade_start(view_range: f32, rnd01: f32) -> f32 {
        view_range * (0.65 + 0.10 * rnd01.clamp(0.0, 1.0))
    }

    /// Fade out in the pixel shader between `start` and `end`: 1 before `start`, 0 after `end`.
    pub fn dissolve(distance: f32, start: f32, end: f32) -> f32 {
        if distance <= start {
            1.0
        } else if distance >= end {
            0.0
        } else {
            1.0 - (distance - start) / (end - start)
        }
    }

    /// Culling sphere radius of `eCVegetation_Grid::Render` (view range x 1, or x 0.45 for a
    /// secondary camera).
    pub fn cull_radius(view_range: f32, secondary_camera: bool) -> f32 {
        view_range * if secondary_camera { 0.45 } else { 1.0 }
    }

    /// Cell index range touching a sphere: the grid node edge is `node_size` (cm).
    pub fn cells_in_radius(centre: [f32; 2], radius: f32, node_size: f32) -> (i32, i32, i32, i32) {
        let f = |v: f32| (v / node_size).floor() as i32;
        (
            f(centre[0] - radius),
            f(centre[1] - radius),
            f(centre[0] + radius),
            f(centre[1] + radius),
        )
    }
}

pub mod shadows {
    //! Shadow map sizes and selection constants (`eCDirectionalLight_PS::UpdateShadowMaps`).

    /// LsPSM camera-following detail map.
    pub const DETAIL_SIZE: u32 = 1408;
    /// Radius of the detail shadow focus box (`ms_fDetailShadowRadius`, cm).
    pub const DETAIL_RADIUS: f32 = 2200.0;
    /// Camera-centred orthographic map.
    pub const STATIC_SIZE: u32 = 1024;
    /// Terrain / tree shadow map.
    pub const TERRAIN_SIZE: u32 = 512;
    /// Point light cube map face size and the number of simultaneous point shadow casters.
    pub const POINT_SIZE: u32 = 512;
    pub const MAX_POINT_CASTERS: usize = 2;
    /// Maximum extent of a static cluster map.
    pub const CLUSTER_EXTENT: f32 = 1700.0;

    /// Caster acceptance of `eCShadowMapDirectionalLight::UpdateInternals` (0x10104610): the
    /// object casts shadows, its light mode is not 2 and its entity carries the cast flag.
    pub fn accepts_caster(cast_shadows: bool, light_mode: u8, entity_flag: bool) -> bool {
        cast_shadows && light_mode != 2 && entity_flag
    }

    /// `shadow * density + (1 - density)`-style strength: the weather `ShadowDensity` scales how
    /// dark the shadowed term gets (1 = full) [I].
    pub fn shadow_strength(visibility: f32, density: f32) -> f32 {
        1.0 - (1.0 - visibility.clamp(0.0, 1.0)) * density.clamp(0.0, 1.0)
    }
}

pub mod decal {
    //! Decal geometry (`eCDecal::ClipPolygonAgainstPlane` 0x1006d600, `ClipPolygon` 0x1006db90,
    //! `ClipMesh` 0x1006ddc0): triangles facing the decal are clipped against the six planes of
    //! the decal box (Sutherland-Hodgman). The shipped game has no content for it: the world holds
    //! no `eCDecal_PS` entity and `g3.efm` no `CreateDecal` command, so nothing is wired.

    /// Plane `n . p = d`; points with `n . p <= d` are inside.
    #[derive(Clone, Copy, Debug)]
    pub struct Plane {
        pub n: [f32; 3],
        pub d: f32,
    }

    fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }

    /// Clip a convex polygon against one plane.
    pub fn clip_polygon_against_plane(poly: &[[f32; 3]], plane: Plane) -> Vec<[f32; 3]> {
        let mut out = Vec::with_capacity(poly.len() + 1);
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            let (da, db) = (dot(plane.n, a) - plane.d, dot(plane.n, b) - plane.d);
            if da <= 0.0 {
                out.push(a);
            }
            if (da < 0.0 && db > 0.0) || (da > 0.0 && db < 0.0) {
                let t = da / (da - db);
                out.push(std::array::from_fn(|k| a[k] + (b[k] - a[k]) * t));
            }
        }
        out
    }

    /// The six planes of an axis aligned decal box (`centre`, half extents).
    pub fn box_planes(centre: [f32; 3], half: [f32; 3]) -> [Plane; 6] {
        std::array::from_fn(|i| {
            let axis = i / 2;
            let sign = if i % 2 == 0 { 1.0 } else { -1.0 };
            let mut n = [0.0; 3];
            n[axis] = sign;
            Plane {
                n,
                d: sign * centre[axis] + half[axis],
            }
        })
    }

    /// Clip a triangle that faces `direction` (the projection direction) against the box; returns
    /// the clipped polygon (empty when culled).
    pub fn clip_triangle(
        tri: [[f32; 3]; 3],
        direction: [f32; 3],
        planes: &[Plane; 6],
    ) -> Vec<[f32; 3]> {
        let e1: [f32; 3] = std::array::from_fn(|k| tri[1][k] - tri[0][k]);
        let e2: [f32; 3] = std::array::from_fn(|k| tri[2][k] - tri[0][k]);
        let n = [
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
        ];
        if dot(direction, n) >= 0.0 {
            return Vec::new();
        }
        let mut poly = tri.to_vec();
        for p in planes {
            poly = clip_polygon_against_plane(&poly, *p);
            if poly.is_empty() {
                break;
            }
        }
        poly
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn clips_a_large_floor_triangle_to_the_box() {
            let planes = box_planes([0.0; 3], [1.0; 3]);
            // Counter-clockwise seen from above: normal +Y, so a decal projecting down (-Y) keeps it.
            let tri = [[-10.0, 0.0, -10.0], [-10.0, 0.0, 30.0], [30.0, 0.0, -10.0]];
            let poly = clip_triangle(tri, [0.0, -1.0, 0.0], &planes);
            assert!(poly.len() >= 3);
            for p in &poly {
                assert!(p.iter().all(|c| c.abs() <= 1.0 + 1e-5), "{p:?}");
            }
            // A back facing triangle is dropped.
            assert!(clip_triangle(tri, [0.0, 1.0, 0.0], &planes).is_empty());
            // A triangle outside the box is clipped away.
            let far = [[50.0, 0.0, 50.0], [50.0, 0.0, 60.0], [60.0, 0.0, 50.0]];
            assert!(clip_triangle(far, [0.0, -1.0, 0.0], &planes).is_empty());
        }
    }
}

pub mod frustum {
    //! Rules of `eCFrustumDatabase` that do not depend on the graphics API.

    /// Entities beyond this projected area factor stop processing their children
    /// (double 3.0e-6 at 0x104888c0).
    pub const CHILD_AREA_MIN: f32 = 3.0e-6;

    /// Element draw lists: an element with a fade below 1 leaves the solid and z-pass lists and
    /// is drawn in the alpha list (`Request`, 0x101a35c0).
    pub fn goes_to_alpha_list(fade: f32) -> bool {
        fade < 1.0
    }

    /// Alpha elements are sorted back to front by camera distance.
    pub fn alpha_sort_key(distance: f32) -> f32 {
        -distance
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn area_factor_values() {
        assert!((lod::area_factor(1.0, 10.0) - std::f32::consts::PI * 0.01).abs() < 1e-6);
        assert_eq!(lod::area_factor(100.0, 1.0), 1.0);
        assert!(lod::area_factor(1.0, 0.0) <= 1.0);
    }

    #[test]
    fn stage_thresholds() {
        assert_eq!(lod::lod_stage(0.3, 4), 0);
        assert_eq!(lod::lod_stage(0.1, 4), 0);
        assert_eq!(lod::lod_stage(0.03, 4), 1);
        assert_eq!(lod::lod_stage(0.005, 4), 2);
        assert_eq!(lod::lod_stage(0.002, 4), 3);
        assert_eq!(lod::lod_stage(0.0001, 4), 3);
        assert_eq!(lod::lod_stage(0.0001, 2), 1);
        assert_eq!(lod::lod_stage(0.03, 0), 0);
        assert_eq!(lod::lod_stage(0.03, 1), 0);
    }

    #[test]
    fn small_object_band() {
        let s = 0.01;
        assert_eq!(cull::small_object_fade(0.0, s), 0.0);
        assert_eq!(cull::small_object_fade(0.5 * s, s), 0.0);
        assert_eq!(cull::small_object_fade(1.1 * s, s), 1.0);
        let mid = cull::small_object_fade(0.8 * s, s);
        assert!((mid - 0.5).abs() < 1e-5, "{mid}");
        assert!(frustum::goes_to_alpha_list(mid));
        assert!((cull::ease(0.0)).abs() < 1e-6 && (cull::ease(1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn distance_band_matches_the_area_rule() {
        let s = cull::screen_cull_factor(0);
        let (near, far) = cull::distance_band(2.0, s);
        assert_eq!(
            cull::small_object_fade(lod::area_factor(2.0, near * 0.999), s),
            1.0
        );
        assert!(cull::small_object_fade(lod::area_factor(2.0, far * 1.001), s) <= 1e-6);
        assert!(far > near);
    }

    #[test]
    fn ranged_window() {
        assert_eq!(cull::ranged_fade(11000.0, 10000.0, 1500.0), 0.0);
        assert_eq!(cull::ranged_fade(5000.0, 10000.0, 1500.0), 1.0);
        let f = cull::ranged_fade(9250.0, 10000.0, 1500.0);
        assert!((f - 0.5).abs() < 1e-5);
    }

    #[test]
    fn static_light_packet_keeps_three_strongest() {
        let mk = |x: f32, i: f32| lights::StaticLight {
            position: [x, 0.0, 0.0],
            color: [i, i, i],
            range: 500.0,
        };
        let p = lights::packet(
            [-10.0; 3],
            [10.0; 3],
            [
                mk(100.0, 1.0),
                mk(10.0, 1.0),
                mk(300.0, 1.0),
                mk(50.0, 1.0),
                mk(5000.0, 9.0),
            ],
        );
        assert_eq!(p.count, 3);
        assert_eq!(p.x[0], 10.0, "nearest first");
        assert_eq!(p.r[0], 2.0, "colour doubled");
        assert!((p.inv_range2[0] - 1.0 / 250_000.0).abs() < 1e-12);
        assert_eq!(lights::packet([0.0; 3], [1.0; 3], []).count, 0);
    }

    #[test]
    fn static_light_weight_and_intensity() {
        let l = lights::StaticLight {
            position: [0.0, 0.0, 0.0],
            color: [1.0, 1.0, 1.0],
            range: 100.0,
        };
        // Luminance weights sum to 1; the weight is the luminance at the light and 0 at the range.
        assert!((lights::weight(&l, [0.0; 3]) - 1.0).abs() < 1e-6);
        assert_eq!(lights::weight(&l, [100.0, 0.0, 0.0]), 0.0);
        assert!((lights::weight(&l, [50.0, 0.0, 0.0]) - 0.75).abs() < 1e-6);
        // A bounding sphere reduces the distance by its radius.
        assert!((lights::weight_sphere(&l, [60.0, 0.0, 0.0], 10.0) - 0.75).abs() < 1e-6);
        let green = lights::StaticLight {
            color: [0.0, 1.0, 0.0],
            ..l
        };
        assert!((lights::weight(&green, [0.0; 3]) - 0.59).abs() < 1e-6);
        // 2 (1 - 3 t^2 + 2 t^3): 0 at the range, 1 at half the range, 2 close by.
        assert_eq!(lights::intensity(100.0, 100.0, 1.0), 0.0);
        assert!((lights::intensity(50.0, 100.0, 1.0) - 1.0).abs() < 1e-6);
        assert!((lights::intensity(1.0, 100.0, 1.0) - 2.0).abs() < 0.01);
        assert_eq!(lights::intensity(150.0, 100.0, 1.0), 0.0);
    }

    #[test]
    fn stage_distances_invert_the_stage_rule() {
        let r = 2.0;
        let d = lod::stage_distances(r, 4, 1.0, 1.0);
        assert_eq!(d.len(), 3);
        assert!(d.windows(2).all(|w| w[0] < w[1]));
        for (i, z) in d.iter().enumerate() {
            let at = |z: f32| lod::lod_input(1.0, lod::area_factor(r, z), 1.0, 1.0);
            assert_eq!(lod::lod_stage(at(z * 0.98), 4), i);
            assert_eq!(lod::lod_stage(at(z * 1.02), 4), i + 1);
        }
        assert!(lod::stage_distances(r, 1, 1.0, 1.0).is_empty());
        // More LoD meshes than table bounds still yield four switches at most.
        assert_eq!(lod::stage_distances(r, 9, 1.0, 1.0).len(), 4);
    }

    #[test]
    fn nearest_lights_for_a_view() {
        let mk = |x: f32| lights::StaticLight {
            position: [x, 0.0, 0.0],
            color: [1.0; 3],
            range: 10.0,
        };
        let sel = lights::select([0.0; 3], 5.0, [mk(1.0), mk(3.0), mk(100.0), mk(14.0)], 2);
        assert_eq!(sel.len(), 2);
        assert_eq!(sel[0].position[0], 1.0);
    }

    #[test]
    fn vegetation_fade() {
        let v = vegetation::view_range(0);
        assert_eq!(v, 7500.0);
        let s = vegetation::fade_start(v, 0.0);
        assert!((s - 4875.0).abs() < 1e-3);
        assert_eq!(vegetation::dissolve(s - 1.0, s, v), 1.0);
        assert_eq!(vegetation::dissolve(v + 1.0, s, v), 0.0);
        assert!((vegetation::dissolve(0.5 * (s + v), s, v) - 0.5).abs() < 1e-5);
        assert_eq!(vegetation::cull_radius(1000.0, true), 450.0);
    }

    #[test]
    fn shadow_rules() {
        assert!(shadows::accepts_caster(true, 1, true));
        assert!(!shadows::accepts_caster(true, 2, true));
        assert_eq!(shadows::shadow_strength(0.0, 0.5), 0.5);
        assert_eq!(shadows::DETAIL_SIZE, 0x580);
    }
}
