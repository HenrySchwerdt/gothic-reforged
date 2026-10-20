//! SpeedTree species construction: real branch/frond/leaf-card geometry generated from the
//! `.spt` parameters ([`gothic_formats::terrain::spt_gen`]) plus the far-distance billboard cut
//! from the species' own rectangle of the composite map.
//!
//! The composite map (`G3_Speedtree_Misc_Composite_NN`) holds the leaf-cluster cards, the frond
//! images and the whole-tree billboard images of several species; the `.spt` stores the texture
//! rectangle of each. The generated skeleton is calibrated against the species billboard
//! silhouette (crown radius and crown base), because the exact length semantics of the
//! generator parameters are only partly known (see `docs/research/SPEEDTREE_FORMAT.md`).
use super::{GeomData, TexData};
use gothic_formats::terrain::{
    spt::Spt,
    spt_gen::{self, GenOptions, MeshOut},
};

/// Number of random variants generated per species.
pub const VARIANTS: usize = 3;

/// Silhouette of the whole-tree billboard sprite inside its composite rectangle.
#[derive(Clone, Copy, Debug)]
pub struct Silhouette {
    /// Sprite bounding box in uv (u0, v0, u1, v1; v from the top).
    pub uv: [f32; 4],
    /// Largest half width as a fraction of the sprite height.
    pub half_width: f32,
    /// Height fraction (from the bottom) below which the crown is narrower than 30% of its
    /// maximum, i.e. the visible trunk.
    pub crown_base: f32,
}

fn block_alpha(fourcc: &[u8; 4], block: &[u8]) -> u32 {
    match fourcc {
        b"DXT5" => {
            let (a0, a1) = (u32::from(block[0]), u32::from(block[1]));
            let mut bits = 0u64;
            for (i, b) in block[2..8].iter().enumerate() {
                bits |= u64::from(*b) << (8 * i);
            }
            let mut sum = 0;
            for k in 0..16 {
                let idx = (bits >> (3 * k)) & 7;
                let a = if a0 > a1 {
                    match idx {
                        0 => a0,
                        1 => a1,
                        n => ((8 - n as u32) * a0 + (n as u32 - 1) * a1) / 7,
                    }
                } else {
                    match idx {
                        0 => a0,
                        1 => a1,
                        6 => 0,
                        7 => 255,
                        n => ((6 - n as u32) * a0 + (n as u32 - 1) * a1) / 5,
                    }
                };
                sum += a;
            }
            sum / 16
        }
        b"DXT3" => {
            let mut sum = 0;
            for b in &block[..8] {
                sum += u32::from(b & 15) * 17 + u32::from(b >> 4) * 17;
            }
            sum / 16
        }
        _ => 255,
    }
}

/// Measure the sprite inside `rect` (the `.spt` corner list, v from the top).
pub fn silhouette(tex: &TexData, rect: &[f32; 8]) -> Option<Silhouette> {
    if !matches!(&tex.fourcc, b"DXT3" | b"DXT5") {
        return None;
    }
    let mip0 = tex.mips.first()?;
    let (bw, bh) = ((tex.width / 4) as usize, (tex.height / 4) as usize);
    if mip0.len() < bw * bh * 16 {
        return None;
    }
    let u0 = rect[2].min(rect[0]).clamp(0.0, 1.0);
    let u1 = rect[2].max(rect[0]).clamp(0.0, 1.0);
    let v0 = rect[5].min(rect[1]).clamp(0.0, 1.0);
    let v1 = rect[5].max(rect[1]).clamp(0.0, 1.0);
    let (x0, x1) = (
        (u0 * bw as f32) as usize,
        ((u1 * bw as f32) as usize).min(bw),
    );
    let (y0, y1) = (
        (v0 * bh as f32) as usize,
        ((v1 * bh as f32) as usize).min(bh),
    );
    if x1 <= x0 + 2 || y1 <= y0 + 2 {
        return None;
    }
    let mut rows: Vec<(usize, usize, usize)> = Vec::new(); // (min x, max x, count) per row
    for y in y0..y1 {
        let (mut lo, mut hi, mut n) = (usize::MAX, 0usize, 0usize);
        for x in x0..x1 {
            let off = (y * bw + x) * 16;
            if block_alpha(&tex.fourcc, &mip0[off..off + 8]) > 60 {
                lo = lo.min(x);
                hi = hi.max(x);
                n += 1;
            }
        }
        rows.push((lo, hi, n));
    }
    let first = rows.iter().position(|r| r.2 > 0)?;
    let last = rows.iter().rposition(|r| r.2 > 0)?;
    let (mut xmin, mut xmax) = (usize::MAX, 0usize);
    let mut wmax = 0usize;
    for r in &rows[first..=last] {
        if r.2 > 0 {
            xmin = xmin.min(r.0);
            xmax = xmax.max(r.1);
            wmax = wmax.max(r.1 - r.0 + 1);
        }
    }
    let height_blocks = (last - first + 1) as f32;
    // Lowest row (from the bottom) that is at least 30% of the maximum width.
    let wide = rows[first..=last]
        .iter()
        .rposition(|r| r.2 > 0 && (r.1 - r.0 + 1) as f32 >= wmax as f32 * 0.3)
        .unwrap_or(0);
    let crown_base = 1.0 - (wide as f32 + 1.0) / height_blocks;
    Some(Silhouette {
        uv: [
            xmin as f32 / bw as f32,
            (y0 + first) as f32 / bh as f32,
            (xmax + 1) as f32 / bw as f32,
            (y0 + last + 1) as f32 / bh as f32,
        ],
        half_width: wmax as f32 * 0.5 / height_blocks,
        crown_base: crown_base.clamp(0.0, 0.7),
    })
}

pub fn hash_str(s: &str) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for b in s.bytes() {
        h ^= u64::from(b.to_ascii_lowercase());
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn geom(id: u64, m: &MeshOut, ao: impl Fn([f32; 3]) -> f32) -> GeomData {
    GeomData {
        id,
        positions: m.positions.clone(),
        normals: m.normals.clone(),
        uvs: m.uvs.clone(),
        indices: m.indices.clone(),
        colors: m
            .positions
            .iter()
            .map(|p| {
                let a = ao(*p);
                [a, a, a, 1.0]
            })
            .collect(),
        tangents: vec![[1.0, 0.0, 0.0, 1.0]; m.positions.len()],
        light: vec![],
    }
}

/// Everything the loader needs for one species.
pub struct Species {
    /// Bark geometry and card geometry per variant (`None` when empty).
    pub near: Vec<(Option<GeomData>, Option<GeomData>)>,
    /// Far LOD: (bark, cards). A whole-tree billboard has no bark; species without a
    /// billboard rectangle in the `.spt` get a reduced real-geometry tree instead.
    pub far: (Option<GeomData>, GeomData),
    pub height: f32,
    /// Trunk collision cylinder (radius, height) in cm, unscaled.
    pub trunk: (f32, f32),
}

/// Build all geometry of a species. `next_id` hands out unique geometry ids.
pub fn build(spt: &Spt, composite: &TexData, next_id: &mut impl FnMut() -> u64) -> Option<Species> {
    let sil = spt.billboard_rect.and_then(|r| silhouette(composite, &r));
    let size = spt.size;
    // Calibrate the branch length scale against the billboard crown radius (or a typical
    // crown for species without a billboard).
    let (target_r, base) = match sil {
        Some(s) => (s.half_width * size, s.crown_base.clamp(0.05, 0.6)),
        None => (0.3 * size, 0.2),
    };
    let base_opt = GenOptions {
        crown_base: Some(base),
        ..GenOptions::default()
    };
    let probe = spt_gen::generate(spt, 0, base_opt);
    let card_half = spt
        .leaves
        .iter()
        .map(|l| l.size[0] * size * 0.5)
        .fold(0.0f32, f32::max);
    let want = (target_r - card_half * 0.8).max(target_r * 0.4);
    let scale = if probe.crown_radius > 1.0 {
        (want / probe.crown_radius).clamp(0.5, 6.0)
    } else {
        1.0
    };
    let opt = GenOptions {
        length_scale: scale,
        ..base_opt
    };
    let shade = |g: &spt_gen::TreeGeometry| {
        let h = g.height.max(1.0);
        let crown_r = g.crown_radius.max(1.0);
        // Cheap fake self shadowing: darker towards the trunk and the crown centre.
        move |p: [f32; 3]| {
            let d = (p[0] * p[0] + p[2] * p[2]).sqrt() / crown_r;
            let hgt = (p[1] / h).clamp(0.0, 1.0);
            (0.6 + 0.4 * d.min(1.0)) * (0.8 + 0.2 * hgt)
        }
    };
    let mut near = Vec::new();
    for v in 0..VARIANTS as u32 {
        let g = spt_gen::generate(spt, v, opt);
        let ao = shade(&g);
        let bark = (!g.bark.indices.is_empty()).then(|| geom(next_id(), &g.bark, |_| 1.0));
        let cards = (!g.cards.indices.is_empty()).then(|| geom(next_id(), &g.cards, ao));
        near.push((bark, cards));
    }
    let far = if let Some(sil) = sil {
        (None, billboard(next_id(), composite, &sil, size))
    } else {
        let g = spt_gen::generate(
            spt,
            7,
            GenOptions {
                max_cards: 120,
                max_branches: 120,
                crossed: false,
                card_scale: 1.8,
                ..opt
            },
        );
        let ao = shade(&g);
        let bark = (!g.bark.indices.is_empty()).then(|| geom(next_id(), &g.bark, |_| 1.0));
        (bark, geom(next_id(), &g.cards, ao))
    };
    let trunk = spt.trunk_capsule_cm().unwrap_or((size * 0.015, size * 0.3));
    Some(Species {
        near,
        far,
        height: size,
        trunk,
    })
}

/// Two crossed quads with the whole-tree sprite, base at the origin, `size` tall.
fn billboard(id: u64, composite: &TexData, sil: &Silhouette, size: f32) -> GeomData {
    let su = sil.uv;
    let sprite_h = size;
    let sprite_w = sprite_h * ((su[2] - su[0]) * composite.width as f32)
        / ((su[3] - su[1]) * composite.height as f32);
    let pad_u = (su[2] - su[0]) * 0.02;
    let pad_v = (su[3] - su[1]) * 0.005;
    let (u0, v0, u1, v1) = (su[0] + pad_u, su[1] + pad_v, su[2] - pad_u, su[3] - pad_v);
    let mut far = GeomData {
        id,
        positions: Vec::new(),
        normals: Vec::new(),
        uvs: Vec::new(),
        indices: Vec::new(),
        colors: Vec::new(),
        tangents: Vec::new(),
        light: Vec::new(),
    };
    for k in 0..2 {
        let a = k as f32 * std::f32::consts::FRAC_PI_2;
        let (s, c) = a.sin_cos();
        let base = far.positions.len() as u32;
        let hw = sprite_w * 0.5 * (1.0 - 0.04);
        for (x, y, u, v) in [
            (-hw, 0.0, u0, v1),
            (hw, 0.0, u1, v1),
            (hw, sprite_h, u1, v0),
            (-hw, sprite_h, u0, v0),
        ] {
            far.positions.push([x * c, y, x * s]);
            far.normals.push([0.0, 1.0, 0.0]);
            far.uvs.push([u, v]);
            far.colors.push([1.0, 1.0, 1.0, 1.0]);
            far.tangents.push([c, 0.0, s, 1.0]);
        }
        far.indices
            .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    far
}
