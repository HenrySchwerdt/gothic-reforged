//! Reader for SpeedTree RT `.spt` tree-definition files (`__IdvSpt_02_`).
//!
//! The files are **procedural definitions** (5-9 kB): a `u32 tag` + payload stream of
//! generator parameters, text Bezier splines and a few texture-rectangle tables; no meshes are
//! stored. Geometry is produced by [`crate::terrain::spt_gen`]. Payload shapes follow the
//! public tag dictionary of the ByroRedux `.spt` notes (same container as Oblivion/FO3),
//! extended for Gothic 3's tail; semantics were derived by correlating the 98 installed files
//! (see `docs/research/SPEEDTREE_FORMAT.md`; entries marked "inferred" there are unverified).
//!
//! Unknown tags end the walk (the remainder is searched for the few tail records needed), so
//! unparsable input yields an error or a partial result, never a panic.
use anyhow::{Result, ensure};
use serde::Serialize;

/// 1 SpeedTree unit (collision objects) = 1 foot.
pub const FOOT_CM: f32 = 30.48;

/// A text `BezierSpline min max var { n  x y tx ty w ... }` curve.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Spline {
    pub min: f32,
    pub max: f32,
    pub var: f32,
    /// `(x, y, tangent_x, tangent_y)` control points sorted by x.
    pub pts: Vec<[f32; 4]>,
}

impl Spline {
    fn parse(text: &str) -> Option<Self> {
        let mut it = text.split_ascii_whitespace();
        if it.next()? != "BezierSpline" {
            return None;
        }
        let min = it.next()?.parse().ok()?;
        let max = it.next()?.parse().ok()?;
        let var = it.next()?.parse().ok()?;
        if it.next()? != "{" {
            return None;
        }
        let n: usize = it.next()?.parse().ok()?;
        if n > 64 {
            return None;
        }
        let mut pts = Vec::with_capacity(n);
        for _ in 0..n {
            let mut v = [0f32; 5];
            for slot in &mut v {
                *slot = it.next()?.parse().ok()?;
            }
            pts.push([v[0], v[1], v[2], v[3]]);
        }
        pts.sort_by(|a, b| a[0].total_cmp(&b[0]));
        Some(Spline { min, max, var, pts })
    }

    /// Curve value (0..1 normally) at `x` in 0..1: cubic Hermite with the stored tangents.
    pub fn eval(&self, x: f32) -> f32 {
        let (Some(first), Some(last)) = (self.pts.first(), self.pts.last()) else {
            return 1.0;
        };
        if x <= first[0] {
            return first[1];
        }
        if x >= last[0] {
            return last[1];
        }
        let i = self.pts.windows(2).position(|w| x < w[1][0]).unwrap_or(0);
        let (a, b) = (self.pts[i], self.pts[i + 1]);
        let h = (b[0] - a[0]).max(1e-6);
        let slope = |p: [f32; 4]| {
            if p[2].abs() < 0.05 {
                0.0
            } else {
                (p[3] / p[2]).clamp(-8.0, 8.0)
            }
        };
        let t = (x - a[0]) / h;
        let (t2, t3) = (t * t, t * t * t);
        (2.0 * t3 - 3.0 * t2 + 1.0) * a[1]
            + (t3 - 2.0 * t2 + t) * h * slope(a)
            + (-2.0 * t3 + 3.0 * t2) * b[1]
            + (t3 - t2) * h * slope(b)
    }

    /// `min + (max - min) * r` for a random `r` in 0..1 (the variance is applied by callers).
    pub fn range(&self, r: f32) -> f32 {
        self.min + (self.max - self.min) * r
    }
}

/// One branch generation level (trunk = level 0). Tag `6000..6017` block.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Level {
    /// `6000..=6007`: distribution, gravity (inferred), disturbance (inferred), ?, length,
    /// radius, radius profile (inferred), start angle in degrees (inferred).
    pub splines: [Spline; 8],
    /// `6008`: length segments.
    pub segments: u32,
    /// `6009`: cross-section sides (1 = flat ribbon).
    pub sides: u32,
    /// `6010`, `6011`: child start / end fraction along this branch (inferred).
    pub child_start: f32,
    pub child_end: f32,
    /// `6012`: number of child branches (inferred).
    pub child_count: f32,
    /// `6013`, `6014`: children per node (min, max) (inferred).
    pub per_node: [f32; 2],
    /// `6016`: level is rendered as flat frond ribbons (inferred).
    pub frond: bool,
    /// `6017`.
    pub profile: Spline,
}

/// One leaf type (`4000..4007` block).
#[derive(Debug, Clone, Default, Serialize)]
pub struct Leaf {
    pub enabled: bool,
    /// `4002`: density (inferred).
    pub density: f32,
    pub texture: String,
    /// `4005`: card width/height as a fraction of the tree size (inferred).
    pub size: [f32; 2],
    /// Composite-map rectangle, `(u, v)` x4 = top-right, top-left, bottom-left, bottom-right,
    /// `v` counted from the top of the image.
    pub rect: [f32; 8],
}

#[derive(Debug, Clone, Copy, Serialize)]
pub enum CollisionShape {
    /// Trunk cylinder: base xyz, radius, height (feet).
    Cylinder {
        base: [f32; 3],
        radius: f32,
        height: f32,
    },
    /// Sphere: centre xyz, radius (feet).
    Sphere { center: [f32; 3], radius: f32 },
    /// Box: centre xyz + 3 extents (feet).
    Box { center: [f32; 3], extent: [f32; 3] },
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Spt {
    /// Tree height in cm (`2001`).
    pub size: f32,
    pub seed: u32,
    pub bark: String,
    /// Every texture name found (leaf/frond/composite), in file order.
    pub textures: Vec<String>,
    pub levels: Vec<Level>,
    pub leaves: Vec<Leaf>,
    /// Frond rectangle in the composite (`10003`).
    pub frond_rect: Option<[f32; 8]>,
    /// Whole-tree billboard rectangle in the composite (after `10004`).
    pub billboard_rect: Option<[f32; 8]>,
    pub composite: Option<String>,
    pub collision: Vec<CollisionShape>,
    /// Wind: `5002` (min/max angle deg, frequency) and `5004` (xyz?) (inferred).
    pub wind: [f32; 6],
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn u32(&mut self) -> Option<u32> {
        let v = u32::from_le_bytes(self.b.get(self.at..self.at + 4)?.try_into().ok()?);
        self.at += 4;
        Some(v)
    }
    fn f32(&mut self) -> Option<f32> {
        self.u32().map(f32::from_bits).filter(|f| f.is_finite())
    }
    fn skip(&mut self, n: usize) -> Option<()> {
        (self.at + n <= self.b.len()).then(|| self.at += n)
    }
    fn string(&mut self) -> Option<&'a str> {
        let n = self.u32()? as usize;
        let s = self.b.get(self.at..self.at.checked_add(n)?)?;
        if n > 4096 {
            return None;
        }
        self.at += n;
        std::str::from_utf8(s).ok()
    }
    fn rect(&mut self) -> Option<[f32; 8]> {
        let mut r = [0f32; 8];
        for v in &mut r {
            *v = self.f32()?;
        }
        Some(r)
    }
}

const BARE: &[u32] = &[
    1001, 1002, 1003, 1004, 1005, 1007, 1008, 1009, 1010, 1011, 1012, 1015, 1016, 1017, 5644, 8000,
    8001, 9000, 9001, 9005, 9006, 10000, 10001, 11000, 11001, 12000, 12001, 13000,
];
const U8: &[u32] = &[2002, 3003, 3006, 3009, 4000, 5006, 6015, 6016, 13007];
const U32: &[u32] = &[
    1006, 1014, 2001, 2003, 2005, 2006, 2007, 3000, 3001, 3002, 3004, 3005, 3007, 3008, 3010, 4002,
    4007, 5005, 6008, 6009, 6010, 6011, 6012, 6013, 6014, 8002, 8004, 8006, 8007, 8008, 9002, 9003,
    9004, 9007, 9008, 9009, 9010, 9011, 9012, 9013, 9014, 11002, 13002, 13003, 13004, 13006, 13009,
    13010, 13011, 13012, 13013,
];
const VEC3: &[u32] = &[4001, 4004, 4005, 4006, 5000, 5001, 5002, 5003, 5004];

impl Spt {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        ensure!(bytes.len() >= 24 && bytes.len() <= 1 << 20, "spt size");
        ensure!(
            bytes.get(8..20) == Some(b"__IdvSpt_02_"),
            "not a SpeedTree spt"
        );
        let mut out = Spt::default();
        let mut r = Reader { b: bytes, at: 20 };
        let mut level: Option<usize> = None;
        let mut have_size = false;
        let mut leaf_rects: Vec<[f32; 8]> = Vec::new();
        'walk: while let Some(tag) = r.u32() {
            let ok: Option<()> = (|| {
                match tag {
                    t if BARE.contains(&t) => {}
                    2000 => {
                        let s = r.string()?.to_owned();
                        out.bark = s;
                    }
                    t if U8.contains(&t) => {
                        let v = *r.b.get(r.at)?;
                        r.skip(1)?;
                        match t {
                            4000 => {
                                out.leaves.push(Leaf {
                                    enabled: v != 0,
                                    ..Leaf::default()
                                });
                            }
                            6016 => {
                                if let Some(l) = level {
                                    out.levels[l].frond = v != 0;
                                }
                            }
                            _ => {}
                        }
                    }
                    t if U32.contains(&t) => {
                        let raw = r.u32()?;
                        let f = f32::from_bits(raw);
                        match t {
                            2001 if !have_size && f.is_finite() && f > 0.0 => {
                                out.size = f;
                                have_size = true;
                            }
                            2005 => out.seed = raw,
                            4002 => {
                                if let Some(l) = out.leaves.last_mut() {
                                    l.density = f;
                                }
                            }
                            6008..=6014 => {
                                if let Some(l) = level {
                                    let lv = &mut out.levels[l];
                                    match t {
                                        6008 => lv.segments = raw.min(64),
                                        6009 => lv.sides = raw.min(64),
                                        6010 => lv.child_start = f,
                                        6011 => lv.child_end = f,
                                        6012 => lv.child_count = f,
                                        6013 => lv.per_node[0] = f,
                                        _ => lv.per_node[1] = f,
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    t if VEC3.contains(&t) => {
                        let v = [r.f32()?, r.f32()?, r.f32()?];
                        match t {
                            4005 => {
                                if let Some(l) = out.leaves.last_mut() {
                                    l.size = [v[0], v[1]];
                                }
                            }
                            5002 => {
                                out.wind[..3].copy_from_slice(&v);
                            }
                            5004 => {
                                out.wind[3..].copy_from_slice(&v);
                            }
                            _ => {}
                        }
                    }
                    8003 | 8005 | 8009 => r.skip(52)?,
                    13008 => r.skip(11)?,
                    12002 => {
                        let v = [r.f32()?, r.f32()?, r.f32()?, r.f32()?];
                        out.collision.push(CollisionShape::Sphere {
                            center: [v[0], v[1], v[2]],
                            radius: v[3],
                        });
                    }
                    12003 => {
                        let v = [r.f32()?, r.f32()?, r.f32()?, r.f32()?, r.f32()?];
                        out.collision.push(CollisionShape::Cylinder {
                            base: [v[0], v[1], v[2]],
                            radius: v[3],
                            height: v[4],
                        });
                    }
                    12004 => {
                        let v = [r.f32()?, r.f32()?, r.f32()?, r.f32()?, r.f32()?, r.f32()?];
                        out.collision.push(CollisionShape::Box {
                            center: [v[0], v[1], v[2]],
                            extent: [v[3], v[4], v[5]],
                        });
                    }
                    4003 => {
                        let s = r.string()?.to_owned();
                        if let Some(l) = out.leaves.last_mut() {
                            l.texture = s.clone();
                        }
                        out.textures.push(s);
                    }
                    6000..=6007 | 6017 => {
                        let s = r.string()?;
                        let sp = Spline::parse(s)?;
                        if tag == 6017 {
                            if let Some(l) = level {
                                out.levels[l].profile = sp;
                            }
                        } else if let Some(l) = level {
                            out.levels[l].splines[(tag - 6000) as usize] = sp;
                        }
                    }
                    13001 => {
                        r.string()?;
                    }
                    13005 => {
                        // Bare, or followed by a length-prefixed curve (then not a tag value).
                        let save = r.at;
                        match r.u32() {
                            Some(n) if (8..=512).contains(&n) && r.string_at(save).is_some() => {
                                r.at = save;
                                r.string()?;
                            }
                            _ => r.at = save,
                        }
                    }
                    10002 | 10003 => {
                        let n = r.u32()? as usize;
                        if n > 64 {
                            return None;
                        }
                        let mut v = Vec::new();
                        for _ in 0..n {
                            v.push(r.rect()?);
                        }
                        if tag == 10002 {
                            leaf_rects = v;
                        } else {
                            out.frond_rect = v.first().copied();
                        }
                    }
                    10004 => {
                        let n = r.u32()?;
                        if (1..=8).contains(&n) {
                            let rc = r.rect()?;
                            out.billboard_rect = Some(rc);
                            r.skip(32 * (n as usize - 1))?;
                        }
                    }
                    _ => return None,
                }
                Some(())
            })();
            if tag == 1016 {
                level = Some(out.levels.len());
                out.levels.push(Level::default());
                continue;
            }
            if ok.is_none() {
                break 'walk;
            }
        }
        // Leaf rectangles are listed in leaf order.
        for (leaf, rect) in out.leaves.iter_mut().zip(&leaf_rects) {
            leaf.rect = *rect;
        }
        // Tail: names of the composite map and other textures (text scan).
        let mut i = r.at.min(bytes.len());
        while i + 5 <= bytes.len() {
            let n = u32::from_le_bytes(bytes[i..i + 4].try_into()?) as usize;
            if (6..=96).contains(&n)
                && let Some(s) = bytes.get(i + 4..i + 4 + n)
                && s.iter().all(|c| c.is_ascii_graphic())
            {
                let name = String::from_utf8_lossy(s).into_owned();
                let lower = name.to_ascii_lowercase();
                if lower.ends_with(".dds") && lower.contains("composite") {
                    out.composite = Some(name);
                    break;
                }
                if lower.ends_with(".tga") && !out.textures.contains(&name) {
                    out.textures.push(name);
                }
                i += 4 + n;
                continue;
            }
            i += 1;
        }
        ensure!(out.size > 0.0, "spt has no size token");
        Ok(out)
    }

    /// Trunk collision cylinder in cm: (radius, height).
    pub fn trunk_capsule_cm(&self) -> Option<(f32, f32)> {
        self.collision.iter().find_map(|c| match c {
            CollisionShape::Cylinder { radius, height, .. } => {
                Some((radius * FOOT_CM, height * FOOT_CM))
            }
            _ => None,
        })
    }
}

impl Reader<'_> {
    fn string_at(&self, at: usize) -> Option<&str> {
        let n = u32::from_le_bytes(self.b.get(at..at + 4)?.try_into().ok()?) as usize;
        let s = self.b.get(at + 4..at + 4 + n)?;
        let s = std::str::from_utf8(s).ok()?;
        s.starts_with("BezierSpline").then_some(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spline_parse_and_eval() {
        let s = Spline::parse(
            "BezierSpline 0\t1\t5\n{\n\n\t2\n\t0 1 0.714831 -0.699297 0.079604\n\t1 0.000782381 0.699609 -0.714526 0.107006\n\n}\n",
        )
        .unwrap();
        assert_eq!(s.pts.len(), 2);
        assert!((s.eval(0.5) - 0.5).abs() < 0.05);
        assert!(s.eval(0.0) > 0.99 && s.eval(1.0) < 0.01);
    }

    #[test]
    fn rejects_garbage() {
        assert!(Spt::parse(&[0u8; 64]).is_err());
        assert!(Spt::parse(b"short").is_err());
    }
}
