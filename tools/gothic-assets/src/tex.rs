//! Texture conversion into what the game's Gothic material shader samples: sRGB DXT1/DXT5
//! diffuse and DXT5nm tangent-space normals (x in alpha, y in green, OpenGL +Y), each with a
//! full mip chain.
use anyhow::Result;
use gothic_formats::remaster::DdsTexture;
use image::{RgbaImage, imageops::FilterType};

/// Largest power of two <= `v`, at least 4.
fn pow2_floor(v: u32) -> u32 {
    let mut p = 4;
    while p * 2 <= v {
        p *= 2;
    }
    p
}

pub fn fit_size(img: &RgbaImage, max_dim: u32) -> RgbaImage {
    let w = pow2_floor(img.width().min(max_dim));
    let h = pow2_floor(img.height().min(max_dim));
    if (w, h) == img.dimensions() {
        img.clone()
    } else {
        image::imageops::resize(img, w, h, FilterType::Lanczos3)
    }
}

fn encode(img: &RgbaImage, fourcc: [u8; 4]) -> Vec<u8> {
    let format = if &fourcc == b"DXT1" {
        texpresso::Format::Bc1
    } else {
        texpresso::Format::Bc3
    };
    let (w, h) = (img.width() as usize, img.height() as usize);
    let mut out = vec![0u8; format.compressed_size(w, h)];
    let params = texpresso::Params {
        algorithm: texpresso::Algorithm::IterativeClusterFit,
        ..Default::default()
    };
    format.compress(img.as_raw(), w, h, params, &mut out);
    out
}

fn chain(base: RgbaImage, fourcc: [u8; 4], renormalize: bool) -> DdsTexture {
    let (width, height) = base.dimensions();
    let mut mips = Vec::new();
    let mut cur = base;
    loop {
        mips.push(encode(&cur, fourcc));
        let (w, h) = cur.dimensions();
        if w <= 4 || h <= 4 {
            break;
        }
        cur = image::imageops::resize(&cur, w / 2, h / 2, FilterType::Triangle);
        if renormalize {
            renormalize_nm(&mut cur);
        }
    }
    DdsTexture {
        width,
        height,
        fourcc,
        mips,
    }
}

/// Diffuse map; `alpha` keeps the alpha channel (DXT5), otherwise DXT1.
pub fn diffuse(img: &RgbaImage, max_dim: u32, alpha: bool, gain: [f32; 3]) -> Result<DdsTexture> {
    let mut base = fit_size(img, max_dim);
    if gain.iter().any(|g| (g - 1.0).abs() > 1e-3) {
        for p in base.pixels_mut() {
            for c in 0..3 {
                p.0[c] = (f32::from(p.0[c]) * gain[c]).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    if !alpha {
        for p in base.pixels_mut() {
            p.0[3] = 255;
        }
    }
    Ok(chain(base, if alpha { *b"DXT5" } else { *b"DXT1" }, false))
}

/// Alpha-tested foliage (DXT5): every mip keeps the share of texels above `cutoff` that the
/// top level has (plain averaging thins grass out with distance), and transparent texels take
/// the colour of nearby opaque ones (no dark fringes once filtering mixes them in).
pub fn diffuse_cutout(img: &RgbaImage, max_dim: u32, cutoff: f32) -> Result<DdsTexture> {
    let mut base = fit_size(img, max_dim);
    bleed(&mut base);
    let coverage = |im: &RgbaImage, s: f32| {
        let n = im.pixels().filter(|p| f32::from(p.0[3]) * s >= cutoff * 255.0).count();
        n as f32 / (im.width() * im.height()) as f32
    };
    let target = coverage(&base, 1.0);
    let (width, height) = base.dimensions();
    let mut mips = vec![encode(&base, *b"DXT5")];
    let mut cur = base;
    while cur.width() > 4 && cur.height() > 4 {
        let (w, h) = (cur.width() / 2, cur.height() / 2);
        cur = image::imageops::resize(&cur, w, h, FilterType::Triangle);
        let (mut lo, mut hi) = (0.25f32, 8.0f32);
        for _ in 0..20 {
            let mid = (lo + hi) * 0.5;
            if coverage(&cur, mid) < target { lo = mid } else { hi = mid }
        }
        let s = (lo + hi) * 0.5;
        let mut out = cur.clone();
        for p in out.pixels_mut() {
            p.0[3] = (f32::from(p.0[3]) * s).round().clamp(0.0, 255.0) as u8;
        }
        mips.push(encode(&out, *b"DXT5"));
    }
    Ok(DdsTexture {
        width,
        height,
        fourcc: *b"DXT5",
        mips,
    })
}

/// Fills the colour of transparent texels from opaque neighbours (repeated 3x3 dilation).
fn bleed(img: &mut RgbaImage) {
    let (w, h) = img.dimensions();
    let mut known: Vec<bool> = img.pixels().map(|p| p.0[3] >= 128).collect();
    for _ in 0..16 {
        let src = img.clone();
        let prev = known.clone();
        let mut changed = false;
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize;
                if prev[i] {
                    continue;
                }
                let (mut acc, mut n) = ([0u32; 3], 0u32);
                for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1), (-1, -1), (1, 1), (-1, 1), (1, -1)] {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                        continue;
                    }
                    let j = (ny as u32 * w + nx as u32) as usize;
                    if prev[j] {
                        let p = src.get_pixel(nx as u32, ny as u32).0;
                        for c in 0..3 {
                            acc[c] += u32::from(p[c]);
                        }
                        n += 1;
                    }
                }
                if n > 0 {
                    let a = img.get_pixel(x, y).0[3];
                    img.put_pixel(x, y, image::Rgba([(acc[0] / n) as u8, (acc[1] / n) as u8, (acc[2] / n) as u8, a]));
                    known[i] = true;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
}

/// glTF normal map (RGB, OpenGL convention) -> DXT5nm.
pub fn normal_map(img: &RgbaImage, max_dim: u32) -> Result<DdsTexture> {
    let mut base = fit_size(img, max_dim);
    for p in base.pixels_mut() {
        let [r, g, _, _] = p.0;
        p.0 = [255, g, 0, r];
    }
    Ok(chain(base, *b"DXT5", true))
}

/// Re-normalises a downsampled DXT5nm image (x in alpha, y in green).
fn renormalize_nm(img: &mut RgbaImage) {
    for p in img.pixels_mut() {
        let x = f32::from(p.0[3]) / 127.5 - 1.0;
        let y = f32::from(p.0[1]) / 127.5 - 1.0;
        let z2 = 1.0 - x * x - y * y;
        let (x, y) = if z2 < 0.0 {
            let l = (x * x + y * y).sqrt();
            (x / l, y / l)
        } else {
            (x, y)
        };
        p.0[3] = ((x + 1.0) * 127.5).round().clamp(0.0, 255.0) as u8;
        p.0[1] = ((y + 1.0) * 127.5).round().clamp(0.0, 255.0) as u8;
    }
}

/// Removes large-scale brightness clouds of a tiling photo texture while keeping fine detail:
/// the low-frequency part (block means on a `grid` x `grid` lattice, interpolated with wrap
/// around so the tile stays seamless) is replaced by the global mean with strength `amount`.
pub fn flatten(img: &mut RgbaImage, grid: u32, amount: f32) {
    let (w, h) = img.dimensions();
    let (bw, bh) = ((w / grid).max(1), (h / grid).max(1));
    let mean = mean_rgb(img, None);
    let mut blocks = vec![[0f32; 3]; (grid * grid) as usize];
    for by in 0..grid {
        for bx in 0..grid {
            let mut acc = [0f64; 3];
            for y in by * bh..((by + 1) * bh).min(h) {
                for x in bx * bw..((bx + 1) * bw).min(w) {
                    let p = img.get_pixel(x, y).0;
                    for c in 0..3 {
                        acc[c] += f64::from(p[c]);
                    }
                }
            }
            let n = f64::from(bw * bh);
            blocks[(by * grid + bx) as usize] = acc.map(|a| (a / n) as f32);
        }
    }
    let at = |x: i64, y: i64| blocks[(y.rem_euclid(grid as i64) * grid as i64 + x.rem_euclid(grid as i64)) as usize];
    for y in 0..h {
        let fy = (y as f32 + 0.5) / bh as f32 - 0.5;
        let (y0, ty) = (fy.floor() as i64, fy - fy.floor());
        for x in 0..w {
            let fx = (x as f32 + 0.5) / bw as f32 - 0.5;
            let (x0, tx) = (fx.floor() as i64, fx - fx.floor());
            let (a, b, c, d) = (at(x0, y0), at(x0 + 1, y0), at(x0, y0 + 1), at(x0 + 1, y0 + 1));
            let p = img.get_pixel_mut(x, y);
            for k in 0..3 {
                let low = (a[k] * (1.0 - tx) + b[k] * tx) * (1.0 - ty) + (c[k] * (1.0 - tx) + d[k] * tx) * ty;
                let v = f32::from(p.0[k]) + (mean[k] - low) * amount;
                p.0[k] = v.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

/// Per-channel histogram matching: maps every channel's distribution onto the reference's,
/// so the new texture takes over the original's palette, contrast and saturation while its
/// own structure (the photographed detail) stays. `amount` blends towards the matched result.
pub fn match_histogram(img: &mut RgbaImage, reference: &RgbaImage, amount: f32) {
    let cdf = |im: &RgbaImage, c: usize| {
        let mut h = [0u64; 256];
        for p in im.pixels() {
            h[p.0[c] as usize] += 1;
        }
        let total: u64 = h.iter().sum();
        let mut acc = 0u64;
        let mut out = [0f64; 256];
        for (i, v) in h.iter().enumerate() {
            acc += v;
            out[i] = acc as f64 / total.max(1) as f64;
        }
        out
    };
    for c in 0..3 {
        let (src, dst) = (cdf(img, c), cdf(reference, c));
        let mut lut = [0u8; 256];
        let mut j = 0usize;
        for i in 0..256 {
            while j < 255 && dst[j] < src[i] {
                j += 1;
            }
            lut[i] = (i as f32 + (j as f32 - i as f32) * amount).round().clamp(0.0, 255.0) as u8;
        }
        for p in img.pixels_mut() {
            p.0[c] = lut[p.0[c] as usize];
        }
    }
}

/// Mean RGB of the opaque texels (optionally masked).
pub fn mean_rgb(img: &RgbaImage, mask: Option<&[bool]>) -> [f32; 3] {
    let (mut sum, mut n) = ([0f64; 3], 0usize);
    for (i, p) in img.pixels().enumerate() {
        if p.0[3] < 128 || mask.is_some_and(|m| !m.get(i).copied().unwrap_or(false)) {
            continue;
        }
        for c in 0..3 {
            sum[c] += f64::from(p.0[c]);
        }
        n += 1;
    }
    sum.map(|s| if n == 0 { 0.0 } else { (s / n as f64) as f32 })
}

/// Per-channel gain mapping the mean colour `from` onto `to`: the luma ratio (0.7..1.8) times a
/// milder chroma correction (each channel within 0.8..1.25 of the luma gain).
pub fn colour_gain(from: [f32; 3], to: [f32; 3]) -> [f32; 3] {
    let luma = |c: [f32; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    if luma(from) <= 0.0 || luma(to) <= 0.0 {
        return [1.0; 3];
    }
    let g = (luma(to) / luma(from)).clamp(0.7, 1.8);
    let mut out = [g; 3];
    for c in 0..3 {
        if from[c] > 1.0 {
            out[c] = g * ((to[c] / from[c]) / (luma(to) / luma(from))).clamp(0.8, 1.25);
        }
    }
    out
}

/// Decodes a DXT mip into RGBA8.
pub fn decode(fourcc: &[u8; 4], blocks: &[u8], w: u32, h: u32) -> Result<RgbaImage> {
    let mut px = vec![0u32; (w * h) as usize];
    match fourcc {
        b"DXT1" => texture2ddecoder::decode_bc1(blocks, w as usize, h as usize, &mut px),
        b"DXT3" => texture2ddecoder::decode_bc2(blocks, w as usize, h as usize, &mut px),
        _ => texture2ddecoder::decode_bc3(blocks, w as usize, h as usize, &mut px),
    }
    .map_err(|e| anyhow::anyhow!("decode: {e}"))?;
    let raw = px
        .iter()
        .flat_map(|p| {
            let [b, g, r, a] = p.to_le_bytes();
            [r, g, b, a]
        })
        .collect();
    RgbaImage::from_raw(w, h, raw).ok_or_else(|| anyhow::anyhow!("size mismatch"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diffuse_chain_reaches_4x4_and_round_trips_colour() -> Result<()> {
        let img = RgbaImage::from_pixel(64, 32, image::Rgba([200, 100, 50, 255]));
        let t = diffuse(&img, 1024, false, [1.0; 3])?;
        assert_eq!((t.width, t.height, &t.fourcc), (64, 32, b"DXT1"));
        assert_eq!(t.mips.len(), 4); // 64x32, 32x16, 16x8, 8x4
        let back = decode(&t.fourcc, &t.mips[0], 64, 32)?;
        let p = back.get_pixel(5, 5).0;
        assert!((i32::from(p[0]) - 200).abs() < 8 && (i32::from(p[2]) - 50).abs() < 8);
        Ok(())
    }

    #[test]
    fn normal_map_moves_x_to_alpha() -> Result<()> {
        let img = RgbaImage::from_pixel(8, 8, image::Rgba([255, 128, 128, 255]));
        let t = normal_map(&img, 8)?;
        let back = decode(&t.fourcc, &t.mips[0], 8, 8)?;
        let p = back.get_pixel(1, 1).0;
        assert!(p[3] > 240 && (i32::from(p[1]) - 128).abs() < 6, "{p:?}");
        Ok(())
    }
}
