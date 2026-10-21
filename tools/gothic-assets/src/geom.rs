//! Small geometry kit: vectors, bounds, surface sampling, closest-point queries and the
//! similarity fit of a generated model onto an original mesh.
use serde::Serialize;

pub type V3 = [f32; 3];

pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub fn scale(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
pub fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub fn len(a: V3) -> f32 {
    dot(a, a).sqrt()
}
pub fn norm(a: V3) -> V3 {
    let l = len(a);
    if l > 1e-12 { scale(a, 1.0 / l) } else { [0.0, 1.0, 0.0] }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Aabb {
    pub min: V3,
    pub max: V3,
}

impl Aabb {
    pub fn of<'a>(points: impl IntoIterator<Item = &'a V3>) -> Self {
        let mut b = Aabb {
            min: [f32::INFINITY; 3],
            max: [f32::NEG_INFINITY; 3],
        };
        for p in points {
            for k in 0..3 {
                b.min[k] = b.min[k].min(p[k]);
                b.max[k] = b.max[k].max(p[k]);
            }
        }
        b
    }
    pub fn size(&self) -> V3 {
        sub(self.max, self.min)
    }
    pub fn center(&self) -> V3 {
        scale(add(self.min, self.max), 0.5)
    }
    pub fn diagonal(&self) -> f32 {
        len(self.size())
    }
}

/// Deterministic area-weighted surface samples (low-discrepancy within each triangle).
pub fn sample_surface(tris: &[[V3; 3]], count: usize) -> Vec<V3> {
    let areas: Vec<f32> = tris
        .iter()
        .map(|t| 0.5 * len(cross(sub(t[1], t[0]), sub(t[2], t[0]))))
        .collect();
    let total: f32 = areas.iter().sum();
    if total <= 0.0 {
        return tris.iter().map(|t| t[0]).collect();
    }
    let mut out = Vec::with_capacity(count + tris.len());
    let mut carry = 0.0f32;
    let mut seq = 0u32;
    for (t, a) in tris.iter().zip(&areas) {
        carry += a / total * count as f32;
        while carry >= 1.0 {
            carry -= 1.0;
            seq += 1;
            // R2 sequence for (u, v), folded into the triangle.
            let mut u = (0.5 + seq as f32 * 0.754_877_7).fract();
            let mut v = (0.5 + seq as f32 * 0.569_840_3).fract();
            if u + v > 1.0 {
                u = 1.0 - u;
                v = 1.0 - v;
            }
            out.push(add(
                t[0],
                add(scale(sub(t[1], t[0]), u), scale(sub(t[2], t[0]), v)),
            ));
        }
    }
    out
}

/// Uniform hash grid over points for nearest-neighbour distance queries.
pub struct PointGrid {
    cell: f32,
    map: std::collections::HashMap<(i32, i32, i32), Vec<V3>>,
}

impl PointGrid {
    pub fn new(points: &[V3], cell: f32) -> Self {
        let mut map: std::collections::HashMap<(i32, i32, i32), Vec<V3>> = Default::default();
        for p in points {
            map.entry(Self::key(*p, cell)).or_default().push(*p);
        }
        Self { cell, map }
    }
    fn key(p: V3, cell: f32) -> (i32, i32, i32) {
        (
            (p[0] / cell).floor() as i32,
            (p[1] / cell).floor() as i32,
            (p[2] / cell).floor() as i32,
        )
    }
    pub fn nearest(&self, p: V3) -> f32 {
        self.nearest_point(p).1
    }

    pub fn nearest_point(&self, p: V3) -> (V3, f32) {
        let (x, y, z) = Self::key(p, self.cell);
        let mut best = f32::INFINITY;
        let mut at = p;
        for r in 0i32..64 {
            for dx in -r..=r {
                for dy in -r..=r {
                    for dz in -r..=r {
                        if dx.abs().max(dy.abs()).max(dz.abs()) != r {
                            continue;
                        }
                        if let Some(v) = self.map.get(&(x + dx, y + dy, z + dz)) {
                            for q in v {
                                let d = len(sub(p, *q));
                                if d < best {
                                    best = d;
                                    at = *q;
                                }
                            }
                        }
                    }
                }
            }
            if best <= r as f32 * self.cell {
                break;
            }
        }
        (at, best)
    }
}

/// Mean symmetric nearest-sample distance between two sample sets.
pub fn chamfer(a: &[V3], b: &[V3], cell: f32) -> f32 {
    let ga = PointGrid::new(a, cell);
    let gb = PointGrid::new(b, cell);
    let da: f32 = a.iter().map(|p| gb.nearest(*p)).sum::<f32>() / a.len().max(1) as f32;
    let db: f32 = b.iter().map(|p| ga.nearest(*p)).sum::<f32>() / b.len().max(1) as f32;
    0.5 * (da + db)
}

/// Closest point on triangle `t` to `p`: (point, barycentric u v w).
pub fn closest_on_triangle(p: V3, t: &[V3; 3]) -> (V3, [f32; 3]) {
    let (a, b, c) = (t[0], t[1], t[2]);
    let ab = sub(b, a);
    let ac = sub(c, a);
    let ap = sub(p, a);
    let d1 = dot(ab, ap);
    let d2 = dot(ac, ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return (a, [1.0, 0.0, 0.0]);
    }
    let bp = sub(p, b);
    let d3 = dot(ab, bp);
    let d4 = dot(ac, bp);
    if d3 >= 0.0 && d4 <= d3 {
        return (b, [0.0, 1.0, 0.0]);
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return (add(a, scale(ab, v)), [1.0 - v, v, 0.0]);
    }
    let cp = sub(p, c);
    let d5 = dot(ab, cp);
    let d6 = dot(ac, cp);
    if d6 >= 0.0 && d5 <= d6 {
        return (c, [0.0, 0.0, 1.0]);
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return (add(a, scale(ac, w)), [1.0 - w, 0.0, w]);
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return (add(b, scale(sub(c, b), w)), [0.0, 1.0 - w, w]);
    }
    let denom = 1.0 / (va + vb + vc);
    let v = vb * denom;
    let w = vc * denom;
    (add(a, add(scale(ab, v), scale(ac, w))), [1.0 - v - w, v, w])
}

/// Barycentric coordinates of `p` projected onto the plane of `t` (may be outside [0, 1]).
pub fn plane_barycentric(p: V3, t: &[V3; 3]) -> [f32; 3] {
    let v0 = sub(t[1], t[0]);
    let v1 = sub(t[2], t[0]);
    let v2 = sub(p, t[0]);
    let d00 = dot(v0, v0);
    let d01 = dot(v0, v1);
    let d11 = dot(v1, v1);
    let d20 = dot(v2, v0);
    let d21 = dot(v2, v1);
    let den = d00 * d11 - d01 * d01;
    if den.abs() < 1e-20 {
        return [1.0, 0.0, 0.0];
    }
    let v = (d11 * d20 - d01 * d21) / den;
    let w = (d00 * d21 - d01 * d20) / den;
    [1.0 - v - w, v, w]
}

/// Hash grid over triangles for closest-point-on-surface queries.
pub struct TriGrid<'a> {
    tris: &'a [[V3; 3]],
    cell: f32,
    map: std::collections::HashMap<(i32, i32, i32), Vec<u32>>,
}

impl<'a> TriGrid<'a> {
    pub fn new(tris: &'a [[V3; 3]], cell: f32) -> Self {
        let mut map: std::collections::HashMap<(i32, i32, i32), Vec<u32>> = Default::default();
        for (i, t) in tris.iter().enumerate() {
            let b = Aabb::of(t.iter());
            let lo = PointGrid::key(b.min, cell);
            let hi = PointGrid::key(b.max, cell);
            for x in lo.0..=hi.0 {
                for y in lo.1..=hi.1 {
                    for z in lo.2..=hi.2 {
                        map.entry((x, y, z)).or_default().push(i as u32);
                    }
                }
            }
        }
        Self { tris, cell, map }
    }

    /// Closest triangle: (triangle index, point, barycentric, distance).
    pub fn closest(&self, p: V3) -> Option<(usize, V3, [f32; 3], f32)> {
        self.closest_filtered(p, |_| true)
    }

    pub fn closest_filtered(
        &self,
        p: V3,
        accept: impl Fn(usize) -> bool,
    ) -> Option<(usize, V3, [f32; 3], f32)> {
        self.closest_within(p, accept, 128)
    }

    /// As [`Self::closest_filtered`], searching at most `rings` grid rings around `p`.
    pub fn closest_within(
        &self,
        p: V3,
        accept: impl Fn(usize) -> bool,
        rings: i32,
    ) -> Option<(usize, V3, [f32; 3], f32)> {
        let (x, y, z) = PointGrid::key(p, self.cell);
        let mut best: Option<(usize, V3, [f32; 3], f32)> = None;
        let mut seen = std::collections::HashSet::new();
        for r in 0i32..rings {
            for dx in -r..=r {
                for dy in -r..=r {
                    for dz in -r..=r {
                        if dx.abs().max(dy.abs()).max(dz.abs()) != r {
                            continue;
                        }
                        let Some(list) = self.map.get(&(x + dx, y + dy, z + dz)) else {
                            continue;
                        };
                        for &i in list {
                            let i = i as usize;
                            if !seen.insert(i) || !accept(i) {
                                continue;
                            }
                            let (q, bary) = closest_on_triangle(p, &self.tris[i]);
                            let d = len(sub(p, q));
                            if best.is_none_or(|b| d < b.3) {
                                best = Some((i, q, bary, d));
                            }
                        }
                    }
                }
            }
            if let Some(b) = best
                && b.3 <= r as f32 * self.cell
            {
                break;
            }
        }
        best
    }
}

/// Similarity transform Gothic-space model -> original frame: yaw (multiples of 90 degrees
/// about Y), uniform scale, translation.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Fit {
    pub yaw_degrees: f32,
    pub scale: f32,
    pub translate: V3,
}

impl Fit {
    pub fn rotate(&self, p: V3) -> V3 {
        let (s, c) = self.yaw_degrees.to_radians().sin_cos();
        [c * p[0] + s * p[2], p[1], -s * p[0] + c * p[2]]
    }
    pub fn point(&self, p: V3) -> V3 {
        add(scale(self.rotate(p), self.scale), self.translate)
    }
    pub fn vector(&self, v: V3) -> V3 {
        self.rotate(v)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaleMode {
    /// Geometric mean of the three extent ratios.
    Volume,
    /// Match the height (Y extent).
    Height,
    /// Match the largest horizontal extent.
    Footprint,
}

#[derive(Debug, Serialize)]
pub struct FitCandidate {
    pub yaw_degrees: f32,
    pub scale: f32,
    /// Chamfer distance relative to the original's bounding-box diagonal.
    pub chamfer_rel: f32,
    /// New / original extent per axis after the fit.
    pub extent_ratio: V3,
}

/// Tries the four yaw candidates (or the forced one) and keeps the best chamfer match.
/// The fitted model is centred on the original bounds in X/Z and shares its floor (min Y).
pub fn fit(
    original: &[[V3; 3]],
    model: &[[V3; 3]],
    mode: ScaleMode,
    yaw: Option<f32>,
    refine: bool,
) -> (Fit, Vec<FitCandidate>) {
    let ob = Aabb::of(original.iter().flatten());
    let os = sample_surface(original, 6000);
    let ms_raw = sample_surface(model, 6000);
    let cell = (ob.diagonal() / 40.0).max(0.5);
    let yaws: Vec<f32> = match yaw {
        Some(y) => vec![y],
        None => vec![0.0, 90.0, 180.0, 270.0],
    };
    let mut cands = Vec::new();
    let mut best: Option<(Fit, f32)> = None;
    for y in yaws {
        let r = Fit {
            yaw_degrees: y,
            scale: 1.0,
            translate: [0.0; 3],
        };
        let rotated: Vec<V3> = model.iter().flatten().map(|p| r.rotate(*p)).collect();
        let mb = Aabb::of(rotated.iter());
        let (o, m) = (ob.size(), mb.size());
        let ratio = |k: usize| if m[k] > 1e-9 { o[k] / m[k] } else { 1.0 };
        let s = match mode {
            ScaleMode::Volume => (ratio(0) * ratio(1) * ratio(2)).cbrt(),
            ScaleMode::Height => ratio(1),
            ScaleMode::Footprint => {
                if o[0].max(o[2]) > 0.0 && m[0].max(m[2]) > 0.0 {
                    o[0].max(o[2]) / m[0].max(m[2])
                } else {
                    ratio(1)
                }
            }
        };
        let mc = mb.center();
        let oc = ob.center();
        let translate = [
            oc[0] - mc[0] * s,
            ob.min[1] - mb.min[1] * s,
            oc[2] - mc[2] * s,
        ];
        let f = Fit {
            yaw_degrees: y,
            scale: s,
            translate,
        };
        let ms: Vec<V3> = ms_raw.iter().map(|p| f.point(*p)).collect();
        let c = chamfer(&os, &ms, cell) / ob.diagonal().max(1e-6);
        let fb = Aabb::of(ms.iter());
        let fs = fb.size();
        cands.push(FitCandidate {
            yaw_degrees: y,
            scale: s,
            chamfer_rel: c,
            extent_ratio: [fs[0] / o[0].max(1e-6), fs[1] / o[1].max(1e-6), fs[2] / o[2].max(1e-6)],
        });
        if best.is_none_or(|b| c < b.1) {
            best = Some((f, c));
        }
    }
    let mut f = best.expect("at least one candidate").0;
    // Translation-only ICP: removes the offset a bounding-box centring leaves on asymmetric
    // shapes (a crooked torch pole, a lean-to on one side of a house).
    if refine {
        let og = PointGrid::new(&os, cell);
        for _ in 0..8 {
            let mut acc = [0.0f64; 3];
            let mut n = 0usize;
            for p in &ms_raw {
                let q = f.point(*p);
                let (o, d) = og.nearest_point(q);
                if d < ob.diagonal() * 0.1 {
                    for k in 0..3 {
                        acc[k] += f64::from(o[k] - q[k]);
                    }
                    n += 1;
                }
            }
            if n == 0 {
                break;
            }
            let step = acc.map(|a| (a / n as f64) as f32);
            f.translate = add(f.translate, step);
            if len(step) < ob.diagonal() * 1e-4 {
                break;
            }
        }
        let ms: Vec<V3> = ms_raw.iter().map(|p| f.point(*p)).collect();
        let c = chamfer(&os, &ms, cell) / ob.diagonal().max(1e-6);
        let fs = Aabb::of(ms.iter()).size();
        let o = ob.size();
        cands.push(FitCandidate {
            yaw_degrees: f.yaw_degrees,
            scale: f.scale,
            chamfer_rel: c,
            extent_ratio: [fs[0] / o[0].max(1e-6), fs[1] / o[1].max(1e-6), fs[2] / o[2].max(1e-6)],
        });
    }
    (f, cands)
}

/// Share of `model` triangles whose face normal agrees with the closest `original` triangle.
pub fn orientation_agreement(original: &[[V3; 3]], model: &[[V3; 3]]) -> f32 {
    let ob = Aabb::of(original.iter().flatten());
    let grid = TriGrid::new(original, (ob.diagonal() / 40.0).max(0.5));
    let fnorm = |t: &[V3; 3]| norm(cross(sub(t[1], t[0]), sub(t[2], t[0])));
    let (mut agree, mut n) = (0usize, 0usize);
    for t in model.iter().step_by((model.len() / 2000).max(1)) {
        let c = scale(add(add(t[0], t[1]), t[2]), 1.0 / 3.0);
        if let Some((i, _, _, _)) = grid.closest(c) {
            n += 1;
            agree += usize::from(dot(fnorm(&original[i]), fnorm(t)) > 0.0);
        }
    }
    agree as f32 / n.max(1) as f32
}

/// Per-vertex tangents in glTF convention (xyz along +U, w so that
/// `cross(normal, tangent) * w` points towards decreasing V, i.e. image "up", which is where
/// OpenGL-style normal maps put +Y).
pub fn tangents(positions: &[V3], normals: &[V3], uvs: &[[f32; 2]], indices: &[u32]) -> Vec<[f32; 4]> {
    let n = positions.len();
    let mut tan = vec![[0.0f32; 3]; n];
    let mut bit = vec![[0.0f32; 3]; n];
    for t in indices.chunks_exact(3) {
        let (i0, i1, i2) = (t[0] as usize, t[1] as usize, t[2] as usize);
        let e1 = sub(positions[i1], positions[i0]);
        let e2 = sub(positions[i2], positions[i0]);
        let du1 = uvs[i1][0] - uvs[i0][0];
        let dv1 = uvs[i1][1] - uvs[i0][1];
        let du2 = uvs[i2][0] - uvs[i0][0];
        let dv2 = uvs[i2][1] - uvs[i0][1];
        let det = du1 * dv2 - du2 * dv1;
        if det.abs() < 1e-12 {
            continue;
        }
        let r = 1.0 / det;
        let sdir = scale(sub(scale(e1, dv2), scale(e2, dv1)), r); // dP/du
        let tdir = scale(sub(scale(e2, du1), scale(e1, du2)), r); // dP/dv
        for &i in &[i0, i1, i2] {
            tan[i] = add(tan[i], sdir);
            bit[i] = add(bit[i], tdir);
        }
    }
    (0..n)
        .map(|i| {
            let nn = normals[i];
            let t = sub(tan[i], scale(nn, dot(nn, tan[i])));
            let t = if len(t) < 1e-9 {
                // any vector perpendicular to the normal
                norm(cross(nn, if nn[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] }))
            } else {
                norm(t)
            };
            // Image up is -dP/dv.
            let up = scale(bit[i], -1.0);
            let w = if dot(cross(nn, t), up) < 0.0 { -1.0 } else { 1.0 };
            [t[0], t[1], t[2], w]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube(s: f32, o: V3) -> Vec<[V3; 3]> {
        let p = |x: f32, y: f32, z: f32| [o[0] + x * s, o[1] + y * s, o[2] + z * s];
        let q = |a: V3, b: V3, c: V3, d: V3| vec![[a, b, c], [a, c, d]];
        let mut t = Vec::new();
        t.extend(q(p(0., 0., 0.), p(1., 0., 0.), p(1., 1., 0.), p(0., 1., 0.)));
        t.extend(q(p(0., 0., 1.), p(1., 0., 1.), p(1., 1., 1.), p(0., 1., 1.)));
        t.extend(q(p(0., 0., 0.), p(0., 1., 0.), p(0., 1., 1.), p(0., 0., 1.)));
        t.extend(q(p(1., 0., 0.), p(1., 1., 0.), p(1., 1., 1.), p(1., 0., 1.)));
        t.extend(q(p(0., 0., 0.), p(1., 0., 0.), p(1., 0., 1.), p(0., 0., 1.)));
        t.extend(q(p(0., 1., 0.), p(1., 1., 0.), p(1., 1., 1.), p(0., 1., 1.)));
        t
    }

    #[test]
    fn fit_recovers_scale_and_offset() {
        let original = cube(100.0, [-50.0, 0.0, -50.0]);
        let model = cube(1.0, [3.0, -0.5, 7.0]);
        let (f, c) = fit(&original, &model, ScaleMode::Volume, None, true);
        assert!((f.scale - 100.0).abs() < 0.5, "{f:?}");
        let best = c.iter().map(|c| c.chamfer_rel).fold(f32::INFINITY, f32::min);
        assert!(best < 0.01, "{c:?}");
        let p = f.point([3.0, -0.5, 7.0]);
        let b = Aabb::of(original.iter().flatten());
        assert!(p[1] - b.min[1] < 1.0);
    }

    #[test]
    fn closest_point_inside_and_on_edges() {
        let t = [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0]];
        let (q, b) = closest_on_triangle([2.0, 2.0, 5.0], &t);
        assert!(len(sub(q, [2.0, 2.0, 0.0])) < 1e-5);
        assert!((b[0] + b[1] + b[2] - 1.0).abs() < 1e-5);
        let (q, _) = closest_on_triangle([-5.0, -5.0, 0.0], &t);
        assert_eq!(q, [0.0, 0.0, 0.0]);
        let tris = vec![t];
        let g = TriGrid::new(&tris, 4.0);
        let (i, _, _, d) = g.closest([30.0, 0.0, 0.0]).expect("hit");
        assert_eq!(i, 0);
        assert!((d - 20.0).abs() < 1e-4);
    }

    #[test]
    fn tangent_handedness_follows_image_up() {
        // Quad in the XY plane facing +Z, U along +X, V growing downwards (-Y).
        let p = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]];
        let n = vec![[0.0, 0.0, 1.0]; 4];
        let uv = vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
        let t = tangents(&p, &n, &uv, &[0, 1, 2, 0, 2, 3]);
        for v in t {
            assert!((v[0] - 1.0).abs() < 1e-5);
            // cross(+Z, +X) = +Y = image up, so w = +1
            assert_eq!(v[3], 1.0);
        }
    }
}
