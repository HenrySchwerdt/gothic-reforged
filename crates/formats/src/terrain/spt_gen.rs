//! Procedural tree geometry from a parsed [`Spt`] (SpeedTree-style generator).
//!
//! The `.spt` stores generation parameters only. This is a re-implementation of the *kind* of
//! algorithm SpeedTree RT used (recursive branch levels with length/radius profile splines,
//! whorled children, gravity and disturbance, flat frond ribbons, leaf-cluster cards), not a
//! bit-exact clone: parameter meanings marked "inferred" in [`crate::terrain::spt`] were
//! recovered by correlation and the visual result is tuned, not verified against the engine.
//! Output is Y-up centimetres, trunk base at the origin.
use super::spt::{Level, Spt};

#[derive(Debug, Default, Clone)]
pub struct MeshOut {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

#[derive(Debug, Default, Clone)]
pub struct TreeGeometry {
    /// Bark tubes; uv tiles the bark texture.
    pub bark: MeshOut,
    /// Frond ribbons and leaf-cluster cards; uv addresses the composite map.
    pub cards: MeshOut,
    pub height: f32,
    /// Horizontal extent of the crown (radius, cm).
    pub crown_radius: f32,
}

/// Generation limits (LOD0).
#[derive(Debug, Clone, Copy)]
pub struct GenOptions {
    pub max_cards: usize,
    pub max_branches: usize,
    /// Cards per leaf cluster (1 = single quad, 2 = crossed).
    pub crossed: bool,
    /// Multiplier for the length of every non-trunk level (calibration to the billboard).
    pub length_scale: f32,
    /// Fraction of the trunk height where the crown starts (overrides the file when set).
    pub crown_base: Option<f32>,
    /// Leaf card size multiplier (reduced LODs use larger, fewer cards).
    pub card_scale: f32,
    /// Diagnostic-only V flip for rendered leaf-card UVs. Does not alter SPT rectangles or
    /// frond/billboard geometry, and is not evidence for the native texture convention.
    pub flip_leaf_render_v: bool,
}

impl Default for GenOptions {
    fn default() -> Self {
        Self {
            max_cards: 500,
            max_branches: 450,
            crossed: true,
            length_scale: 1.0,
            crown_base: None,
            card_scale: 1.0,
            flip_leaf_render_v: false,
        }
    }
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 40) as f32) / 16_777_216.0
    }
    fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.next()
    }
}

type V3 = [f32; 3];
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn mul(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm(a: V3) -> V3 {
    let l = dot(a, a).sqrt();
    if l < 1e-6 {
        [0.0, 1.0, 0.0]
    } else {
        mul(a, 1.0 / l)
    }
}
/// Any unit vector perpendicular to `d`.
fn perp(d: V3) -> V3 {
    let a = if d[1].abs() < 0.9 {
        [0.0, 1.0, 0.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    norm(cross(d, a))
}
fn rotate(v: V3, axis: V3, ang: f32) -> V3 {
    let (s, c) = ang.sin_cos();
    add(
        add(mul(v, c), mul(cross(axis, v), s)),
        mul(axis, dot(axis, v) * (1.0 - c)),
    )
}

struct Branch {
    pts: Vec<V3>,
    radii: Vec<f32>,
    level: usize,
    length: f32,
}

fn level_value(l: &Level, slot: usize, rng: &mut Rng) -> f32 {
    let s = &l.splines[slot];
    s.range(rng.next()) + s.var * (rng.next() - 0.5) * 2.0
}

/// Build one tree. `variant` selects the random stream.
pub fn generate(spt: &Spt, variant: u32, opt: GenOptions) -> TreeGeometry {
    let mut rng = Rng(u64::from(spt.seed).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (u64::from(variant) + 1).wrapping_mul(0xD1B5_4A32_D192_ED03)
        | 1);
    let size = spt.size;
    let mut branches: Vec<Branch> = Vec::new();
    if spt.levels.is_empty() {
        return TreeGeometry::default();
    }
    // Level 0: trunk.
    let l0 = &spt.levels[0];
    let trunk_r = (size * level_value(l0, 5, &mut rng)).max(0.5);
    let trunk = grow(
        l0,
        [0.0; 3],
        [0.0, 1.0, 0.0],
        size,
        trunk_r,
        0,
        &mut rng,
        1.0,
    );
    branches.push(trunk);
    // Deeper levels.
    let mut first_of_level = vec![0usize];
    for lv in 1..spt.levels.len() {
        let parent_level = &spt.levels[lv - 1];
        let level = &spt.levels[lv];
        let start = first_of_level[lv - 1];
        let end = branches.len();
        first_of_level.push(end);
        if end == start {
            break;
        }
        let mut total = parent_level.child_count.round().max(0.0) as usize;
        total = total.min(opt.max_branches);
        if total == 0 {
            continue;
        }
        let parents = end - start;
        let mut made = 0usize;
        let mut golden = rng.next() * std::f32::consts::TAU;
        let per_node_avg = ((parent_level.per_node[0] + parent_level.per_node[1]) * 0.5).max(1.0);
        // Children are spread evenly over the parents; one parent's children form whorls.
        let mut counts = vec![0usize; parents];
        for m in 0..total {
            counts[m * parents / total] += 1;
        }
        let mut seen = vec![0usize; parents];
        let mut parent_start = parent_level.child_start;
        if lv == 1
            && let Some(cb) = opt.crown_base
        {
            parent_start = cb;
        }
        let (cs, ce) = (
            parent_start.clamp(0.0, 1.0),
            parent_level
                .child_end
                .clamp(0.0, 1.0)
                .max(parent_start.clamp(0.0, 1.0) + 0.02),
        );
        while made < total {
            let slot = made * parents / total;
            let pi = start + slot;
            let k = seen[slot];
            seen[slot] += 1;
            let nodes = (counts[slot] as f32 / per_node_avg).ceil().max(1.0);
            let node = (k as f32 / per_node_avg).floor();
            let t =
                (cs + (ce - cs) * ((node + 0.5 + rng.range(-0.3, 0.3)) / nodes)).clamp(0.0, 0.999);
            let parent = &branches[pi];
            let (pos, dir, pr) = sample(parent, t);
            golden += 2.399_963;
            let len_curve = level.splines[4].eval(t);
            let mut len =
                size * opt.length_scale * level.splines[4].range(rng.next()) * len_curve.max(0.15);
            if level.splines[4].max <= 0.0 {
                len = 0.0;
            }
            if len < 0.5 {
                made += 1;
                continue;
            }
            let ang_curve = level.splines[7].eval(t);
            let angle = (level.splines[7].min
                + (level.splines[7].max - level.splines[7].min) * ang_curve
                + level.splines[7].var * (rng.next() - 0.5) * 2.0)
                .clamp(-170.0, 170.0);
            let angle = if level.splines[7].pts.len() <= 2
                && level.splines[7].min == level.splines[7].max
            {
                level.splines[7].min
            } else {
                angle
            };
            // `90` means perpendicular to the parent axis.
            let a = angle.to_radians().clamp(0.15, 3.0);
            let side = rotate(perp(dir), dir, golden);
            let cdir = norm(add(mul(dir, a.cos()), mul(side, a.sin())));
            let mut r0 = size * level_value(level, 5, &mut rng);
            r0 = r0.min(pr * 0.7).max(0.15);
            let b = grow(level, pos, cdir, len, r0, lv, &mut rng, 1.0);
            branches.push(b);
            made += 1;
        }
    }

    // Geometry.
    let mut out = TreeGeometry {
        height: size,
        ..TreeGeometry::default()
    };
    let leaf_level = leaf_level_index(spt);
    let mut leaf_sites: Vec<(V3, V3, f32)> = Vec::new();
    for b in &branches {
        let lvl = &spt.levels[b.level];
        if lvl.frond && b.level > 0 {
            emit_frond(b, spt, &mut out.cards, &mut rng);
        } else if b.level < 3 && b.radii[0] > 0.25 && (b.level == 0 || lvl.sides > 1) {
            emit_tube(b, lvl.sides.clamp(3, 8), &mut out.bark);
        }
        if Some(b.level) == leaf_level {
            let n = b.pts.len();
            let ends = [n - 1, n / 2];
            for &i in &ends {
                let d = if i + 1 < n {
                    sub(b.pts[i + 1], b.pts[i])
                } else {
                    sub(b.pts[i], b.pts[i - 1])
                };
                leaf_sites.push((b.pts[i], norm(d), b.length));
            }
        }
        for p in &b.pts {
            out.crown_radius = out.crown_radius.max((p[0] * p[0] + p[2] * p[2]).sqrt());
        }
    }
    let leaf_uv_start = out.cards.uvs.len();
    emit_leaves(spt, &leaf_sites, &mut out, &mut rng, opt);
    if opt.flip_leaf_render_v {
        for uv in &mut out.cards.uvs[leaf_uv_start..] {
            uv[1] = 1.0 - uv[1];
        }
    }
    out
}

/// The level whose branches carry leaf clusters: the deepest level that exists in the tree
/// (fronds carry nothing) when leaves are configured.
fn leaf_level_index(spt: &Spt) -> Option<usize> {
    let any = spt.leaves.iter().any(|l| l.rect[0] != l.rect[2]);
    if !any {
        return None;
    }
    let n = spt.levels.len();
    // The last level of every sample file holds the leaves; frond-ribbon levels never do.
    (0..n).rev().find(|&i| i > 0 && !spt.levels[i].frond)
}

#[allow(clippy::too_many_arguments)]
fn grow(
    l: &Level,
    origin: V3,
    dir: V3,
    length: f32,
    r0: f32,
    level: usize,
    rng: &mut Rng,
    _scale: f32,
) -> Branch {
    let segs = (l.segments as usize).clamp(2, 24);
    let seg_len = length / segs as f32;
    let gravity = level_value(l, 1, rng);
    let disturb = level_value(l, 2, rng).abs();
    let mut pts = vec![origin];
    let mut radii = vec![r0 * flare(0.0, level)];
    let mut d = dir;
    for i in 0..segs {
        let t = (i + 1) as f32 / segs as f32;
        let jitter = [
            rng.range(-1.0, 1.0),
            rng.range(-1.0, 1.0),
            rng.range(-1.0, 1.0),
        ];
        d = norm(add(
            d,
            mul(jitter, disturb * if level == 0 { 0.35 } else { 0.5 }),
        ));
        let g = if level == 0 { 0.0 } else { gravity * 0.22 * t };
        d = norm(add(d, [0.0, -g, 0.0]));
        if level == 0 {
            // Keep the trunk upright.
            d = norm(add(d, [0.0, 0.35, 0.0]));
        }
        let p = add(*pts.last().expect("non-empty"), mul(d, seg_len));
        pts.push(p);
        let prof = l.splines[6].eval(t).clamp(0.0, 1.0);
        radii.push((r0 * prof * flare(t, level)).max(r0 * 0.02));
    }
    Branch {
        pts,
        radii,
        level,
        length,
    }
}

fn flare(t: f32, level: usize) -> f32 {
    if level == 0 {
        1.0 + 0.7 * (1.0 - t / 0.08).max(0.0)
    } else {
        1.0
    }
}

/// Position, direction and radius at fraction `t` along `b`.
fn sample(b: &Branch, t: f32) -> (V3, V3, f32) {
    let n = b.pts.len() - 1;
    let f = t.clamp(0.0, 1.0) * n as f32;
    let i = (f.floor() as usize).min(n - 1);
    let u = f - i as f32;
    let p = add(mul(b.pts[i], 1.0 - u), mul(b.pts[i + 1], u));
    let r = b.radii[i] * (1.0 - u) + b.radii[i + 1] * u;
    (p, norm(sub(b.pts[i + 1], b.pts[i])), r)
}

fn emit_tube(b: &Branch, sides: u32, m: &mut MeshOut) {
    let sides = sides as usize;
    let base = m.positions.len() as u32;
    let mut dist = 0.0;
    let circ = (b.radii[0] * std::f32::consts::TAU).max(10.0);
    let reps = (circ / 60.0).round().max(1.0);
    let mut frame_ref = perp(norm(sub(b.pts[1], b.pts[0])));
    for (i, p) in b.pts.iter().enumerate() {
        let d = if i + 1 < b.pts.len() {
            sub(b.pts[i + 1], *p)
        } else {
            sub(*p, b.pts[i - 1])
        };
        let d = norm(d);
        let side = norm(cross(d, cross(frame_ref, d)));
        frame_ref = side;
        let up = cross(d, side);
        if i > 0 {
            dist += dot(sub(*p, b.pts[i - 1]), sub(*p, b.pts[i - 1])).sqrt();
        }
        for k in 0..=sides {
            let a = k as f32 / sides as f32 * std::f32::consts::TAU;
            let n = add(mul(side, a.cos()), mul(up, a.sin()));
            m.positions.push(add(*p, mul(n, b.radii[i])));
            m.normals.push(n);
            m.uvs.push([k as f32 / sides as f32 * reps, dist / 60.0]);
        }
    }
    let ring = (sides + 1) as u32;
    for i in 0..(b.pts.len() as u32 - 1) {
        for k in 0..sides as u32 {
            let a = base + i * ring + k;
            let (b0, c, dd) = (a + 1, a + ring, a + ring + 1);
            m.indices.extend([a, b0, c, b0, dd, c]);
        }
    }
}

fn emit_frond(b: &Branch, spt: &Spt, m: &mut MeshOut, rng: &mut Rng) {
    let Some(rect) = spt.frond_rect else { return };
    let (u0, u1) = (rect[2].min(rect[0]), rect[2].max(rect[0]));
    let (v0, v1) = (rect[5].min(rect[1]), rect[5].max(rect[1]));
    let n = b.pts.len();
    let d0 = norm(sub(b.pts[n - 1], b.pts[0]));
    let side = rotate(perp(d0), d0, rng.next() * std::f32::consts::TAU);
    let base = m.positions.len() as u32;
    let halfw = (b.length * 0.42).max(2.0);
    let nrm = norm(add(mul(norm(cross(d0, side)), 0.3), [0.0, 1.0, 0.0]));
    for (i, p) in b.pts.iter().enumerate() {
        let t = i as f32 / (n - 1) as f32;
        // Envelope: widest in the first third, tapering to the tip.
        let w = halfw * (1.0 - (t - 0.35).abs().powf(1.6) * 0.9).clamp(0.2, 1.0);
        let v = v1 - (v1 - v0) * t;
        m.positions.push(add(*p, mul(side, -w)));
        m.positions.push(add(*p, mul(side, w)));
        m.normals.push(nrm);
        m.normals.push(nrm);
        m.uvs.push([u0, v]);
        m.uvs.push([u1, v]);
    }
    for i in 0..(n as u32 - 1) {
        let a = base + i * 2;
        m.indices.extend([a, a + 2, a + 1, a + 1, a + 2, a + 3]);
    }
}

fn emit_leaves(
    spt: &Spt,
    sites: &[(V3, V3, f32)],
    out: &mut TreeGeometry,
    rng: &mut Rng,
    opt: GenOptions,
) {
    let leaves: Vec<_> = spt
        .leaves
        .iter()
        .filter(|l| l.rect[0] != l.rect[2])
        .collect();
    if leaves.is_empty() || sites.is_empty() {
        return;
    }
    // Crown centre for outward-pointing normals.
    let mut c = [0.0f32; 3];
    for s in sites {
        c = add(c, s.0);
    }
    c = mul(c, 1.0 / sites.len() as f32);
    let budget = opt.max_cards / if opt.crossed { 2 } else { 1 };
    let step = (sites.len() as f32 / budget as f32).max(1.0);
    let mut idx = 0.0f32;
    while (idx as usize) < sites.len() {
        let (p, d, _) = sites[idx as usize];
        idx += step;
        let leaf = leaves[(rng.next() * leaves.len() as f32) as usize % leaves.len()];
        let (w, h) = (
            leaf.size[0] * spt.size * opt.card_scale,
            leaf.size[1] * spt.size * opt.card_scale,
        );
        let (w, h) = (w.max(3.0), h.max(3.0));
        let yaw = rng.next() * std::f32::consts::PI;
        let n = norm(add(mul(norm(sub(p, c)), 0.5), [0.0, 1.0, 0.0]));
        let quads = if opt.crossed { 2 } else { 1 };
        for q in 0..quads {
            let a = yaw + q as f32 * std::f32::consts::FRAC_PI_2;
            let right = [a.cos(), 0.0, a.sin()];
            // Cards hang slightly along the branch direction.
            let up = norm(add([0.0, 1.0, 0.0], mul(d, 0.35)));
            let centre = add(p, mul(d, h * 0.15));
            let (u0, u1) = (
                leaf.rect[2].min(leaf.rect[0]),
                leaf.rect[2].max(leaf.rect[0]),
            );
            let (v0, v1) = (
                leaf.rect[5].min(leaf.rect[1]),
                leaf.rect[5].max(leaf.rect[1]),
            );
            let pad = (u1 - u0) * 0.01;
            let base = out.cards.positions.len() as u32;
            for (sx, sy, u, v) in [
                (-1.0, -1.0, u0 + pad, v1 - pad),
                (1.0, -1.0, u1 - pad, v1 - pad),
                (1.0, 1.0, u1 - pad, v0 + pad),
                (-1.0, 1.0, u0 + pad, v0 + pad),
            ] {
                let pos = add(centre, add(mul(right, sx * w * 0.5), mul(up, sy * h * 0.5)));
                out.cards.positions.push(pos);
                out.cards.normals.push(n);
                out.cards.uvs.push([u, v]);
            }
            out.cards
                .indices
                .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::spt::{Leaf, Spline};

    fn mixed_frond_leaf_fixture() -> Spt {
        let mut levels = vec![Level::default(); 3];
        for level in &mut levels {
            level.segments = 2;
            level.splines[4] = Spline {
                min: 0.35,
                max: 0.35,
                ..Spline::default()
            };
            level.child_start = 0.0;
            level.child_end = 1.0;
            level.per_node = [1.0, 1.0];
        }
        levels[0].child_count = 1.0;
        levels[1].child_count = 1.0;
        levels[1].frond = true;

        Spt {
            size: 1000.0,
            seed: 41,
            levels,
            leaves: vec![Leaf {
                enabled: true,
                size: [0.08, 0.12],
                rect: [0.25, 0.25, 0.0, 0.25, 0.0, 0.0, 0.25, 0.0],
                ..Leaf::default()
            }],
            frond_rect: Some([0.5, 1.0, 0.0, 1.0, 0.0, 0.75, 0.5, 0.75]),
            ..Spt::default()
        }
    }

    #[test]
    fn leaf_render_v_flip_changes_only_leaf_uv_suffix() {
        let spt = mixed_frond_leaf_fixture();
        let options = GenOptions {
            max_cards: 2,
            crossed: false,
            ..GenOptions::default()
        };
        let baseline = generate(&spt, 3, options);
        let flipped = generate(
            &spt,
            3,
            GenOptions {
                flip_leaf_render_v: true,
                ..options
            },
        );
        let mut frond_only = spt.clone();
        frond_only.leaves.clear();
        let fronds = generate(&frond_only, 3, options);
        let leaf_start = fronds.cards.uvs.len();

        assert!(leaf_start > 0, "fixture must generate a frond prefix");
        assert!(
            baseline.cards.uvs.len() > leaf_start,
            "fixture must generate leaf cards"
        );
        assert_eq!(baseline.cards.positions, flipped.cards.positions);
        assert_eq!(baseline.cards.normals, flipped.cards.normals);
        assert_eq!(baseline.cards.indices, flipped.cards.indices);
        assert_eq!(baseline.bark.positions, flipped.bark.positions);
        assert_eq!(baseline.bark.normals, flipped.bark.normals);
        assert_eq!(baseline.bark.uvs, flipped.bark.uvs);
        assert_eq!(baseline.bark.indices, flipped.bark.indices);
        assert_eq!(baseline.height, flipped.height);
        assert_eq!(baseline.crown_radius, flipped.crown_radius);
        assert_eq!(
            baseline.cards.uvs[..leaf_start],
            flipped.cards.uvs[..leaf_start]
        );
        for (before, after) in baseline.cards.uvs[leaf_start..]
            .iter()
            .zip(&flipped.cards.uvs[leaf_start..])
        {
            assert_eq!(before[0], after[0]);
            assert_eq!(after[1], 1.0 - before[1]);
        }
    }
}
