//! Stand-in SpeedTree rendering: whole-tree billboard sprites cut out of the game's
//! `G3_Speedtree_Misc_Composite_*` atlases, placed as three crossed quads.
//!
//! The `.spt` generator is not reimplemented. The atlases hold both leaf-cluster cards and
//! small whole-tree silhouettes; sprites are found by flood-filling the atlas alpha channel
//! (one sample per 4x4 DXT block), and the whole-tree ones (small, taller than wide, well filled)
//! are used. Which atlas a species uses is read from its `.spt`; the sprite within the atlas is
//! picked by hashing the species name (the per-species atlas rectangle is not decoded).
use super::{GeomData, TexData};

#[derive(Clone, Copy, Debug)]
pub struct Sprite {
    /// u0, v0, u1, v1 in 0..1 (top-left origin).
    pub uv: [f32; 4],
    /// Width / height in texels.
    pub aspect: f32,
    pub fill: f32,
    /// Fraction of the sprite width occupied at 15% from its top (small = conical crown).
    pub top_width: f32,
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

/// Connected opaque regions of an atlas, in texture-space rectangles.
pub fn find_sprites(tex: &TexData) -> Vec<Sprite> {
    let (w, h) = (tex.width as usize, tex.height as usize);
    let Some(mip0) = tex.mips.first() else {
        return Vec::new();
    };
    if !matches!(&tex.fourcc, b"DXT3" | b"DXT5") || w < 64 || h < 64 {
        return Vec::new();
    }
    let (bw, bh) = (w / 4, h / 4);
    if mip0.len() < bw * bh * 16 {
        return Vec::new();
    }
    let mask: Vec<bool> = (0..bw * bh)
        .map(|i| block_alpha(&tex.fourcc, &mip0[i * 16..i * 16 + 8]) > 40)
        .collect();
    let mut seen = vec![false; bw * bh];
    let mut out = Vec::new();
    let mut stack = Vec::new();
    for start in 0..bw * bh {
        if !mask[start] || seen[start] {
            continue;
        }
        seen[start] = true;
        stack.push(start);
        let (mut x0, mut y0, mut x1, mut y1, mut count) = (bw, bh, 0usize, 0usize, 0usize);
        let mut members = Vec::new();
        while let Some(i) = stack.pop() {
            let (x, y) = (i % bw, i / bw);
            x0 = x0.min(x);
            x1 = x1.max(x);
            y0 = y0.min(y);
            y1 = y1.max(y);
            count += 1;
            members.push(i);
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx < 0 || ny < 0 || nx >= bw as i32 || ny >= bh as i32 {
                        continue;
                    }
                    let n = ny as usize * bw + nx as usize;
                    if mask[n] && !seen[n] {
                        seen[n] = true;
                        stack.push(n);
                    }
                }
            }
        }
        if count < 24 {
            continue;
        }
        let (sw, sh) = (x1 - x0 + 1, y1 - y0 + 1);
        let row = y0 + sh * 15 / 100;
        let top_count = members
            .iter()
            .filter(|&&i| i / bw == row)
            .count()
            .max(1);
        out.push(Sprite {
            uv: [
                x0 as f32 / bw as f32,
                y0 as f32 / bh as f32,
                (x1 + 1) as f32 / bw as f32,
                (y1 + 1) as f32 / bh as f32,
            ],
            aspect: sw as f32 / sh as f32,
            fill: count as f32 / (sw * sh) as f32,
            top_width: top_count as f32 / sw as f32,
        });
    }
    out
}

/// Whole-tree silhouettes: small (relative to the atlas), upright and reasonably solid.
pub fn tree_sprites(sprites: &[Sprite], atlas_w: u32) -> Vec<Sprite> {
    let max_w = 0.17;
    sprites
        .iter()
        .copied()
        .filter(|s| {
            let width = s.uv[2] - s.uv[0];
            let _ = atlas_w;
            width < max_w && s.aspect < 1.3 && s.aspect > 0.2 && s.fill > 0.16
        })
        .collect()
}

pub fn hash_str(s: &str) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for b in s.bytes() {
        h ^= u64::from(b.to_ascii_lowercase());
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Three crossed quads textured with `sprite`, `height_cm` tall, base at the origin.
/// Mirrored-space geometry (symmetric, so no extra flip is required).
pub fn crossed_billboards(id: u64, sprite: &Sprite, height_cm: f32) -> GeomData {
    let half_w = 0.5 * height_cm * sprite.aspect;
    // Inset the uv rectangle slightly so mip filtering does not bleed neighbouring sprites.
    let pad_u = (sprite.uv[2] - sprite.uv[0]) * 0.02;
    let pad_v = (sprite.uv[3] - sprite.uv[1]) * 0.01;
    let (u0, v0, u1, v1) = (
        sprite.uv[0] + pad_u,
        sprite.uv[1] + pad_v,
        sprite.uv[2] - pad_u,
        sprite.uv[3] - pad_v,
    );
    let mut g = GeomData {
        id,
        positions: Vec::new(),
        normals: Vec::new(),
        uvs: Vec::new(),
        indices: Vec::new(),
        colors: Vec::new(),
        tangents: Vec::new(),
    };
    for k in 0..3 {
        let a = k as f32 * std::f32::consts::PI / 3.0;
        let (s, c) = a.sin_cos();
        let base = g.positions.len() as u32;
        for (x, y, u, v) in [
            (-half_w, 0.0, u0, v1),
            (half_w, 0.0, u1, v1),
            (half_w, height_cm, u1, v0),
            (-half_w, height_cm, u0, v0),
        ] {
            g.positions.push([x * c, y, x * s]);
            g.normals.push([0.0, 1.0, 0.0]);
            g.uvs.push([u, v]);
            g.colors.push([1.0, 1.0, 1.0, 1.0]);
            g.tangents.push([c, 0.0, s, 1.0]);
        }
        g.indices
            .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    g
}
