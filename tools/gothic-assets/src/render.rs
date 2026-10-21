//! Minimal CPU rasteriser for reference views (orthographic, textured, Lambert + ambient,
//! plain background): input sheets for multi-view generation and offline before/after checks.
use crate::geom::{self, Aabb, V3};
use image::{Rgba, RgbaImage};

pub struct Part<'a> {
    /// Gothic space (left-handed); the renderer un-mirrors X so views match the game.
    pub positions: &'a [V3],
    pub normals: &'a [V3],
    pub uvs: &'a [[f32; 2]],
    pub indices: &'a [u32],
    pub texture: Option<&'a RgbaImage>,
    pub alpha_test: bool,
}

fn sample(img: &RgbaImage, uv: [f32; 2]) -> [f32; 4] {
    let x = (uv[0].rem_euclid(1.0) * img.width() as f32) as u32 % img.width();
    let y = (uv[1].rem_euclid(1.0) * img.height() as f32) as u32 % img.height();
    img.get_pixel(x, y).0.map(f32::from)
}

/// Renders `parts` seen from azimuth `azimuth_deg` (0 = looking along +Z of the right-handed
/// frame, i.e. at the model's -Z side) and elevation `elevation_deg`.
pub fn view(parts: &[Part], size: u32, azimuth_deg: f32, elevation_deg: f32, background: [u8; 3]) -> RgbaImage {
    let (sa, ca) = azimuth_deg.to_radians().sin_cos();
    let (se, ce) = elevation_deg.to_radians().sin_cos();
    // Camera basis (right-handed world, Y up): forward points from camera to target.
    let fwd = [sa * ce, -se, ca * ce];
    let right = geom::norm(geom::cross(fwd, [0.0, 1.0, 0.0]));
    let up = geom::cross(right, fwd);
    let mirror = |p: V3| [-p[0], p[1], p[2]];
    let all: Vec<V3> = parts.iter().flat_map(|p| p.positions.iter().map(|v| mirror(*v))).collect();
    let b = Aabb::of(all.iter());
    let c = b.center();
    let radius = b.diagonal() * 0.5 * 1.05;
    let s = size as f32;
    let project = |p: V3| {
        let d = geom::sub(p, c);
        let x = geom::dot(d, right) / radius;
        let y = geom::dot(d, up) / radius;
        let z = geom::dot(d, fwd);
        [(x * 0.5 + 0.5) * s, (0.5 - y * 0.5) * s, z]
    };
    let light = geom::norm(geom::add(geom::scale(fwd, -1.0), geom::add(geom::scale(up, 0.8), geom::scale(right, -0.5))));
    let mut img = RgbaImage::from_pixel(size, size, Rgba([background[0], background[1], background[2], 255]));
    let mut depth = vec![f32::INFINITY; (size * size) as usize];
    for part in parts {
        let pts: Vec<[f32; 3]> = part.positions.iter().map(|p| project(mirror(*p))).collect();
        for t in part.indices.chunks_exact(3) {
            let i = [t[0] as usize, t[1] as usize, t[2] as usize];
            if i.iter().any(|&k| k >= pts.len()) {
                continue;
            }
            let (a, bb, cc) = (pts[i[0]], pts[i[1]], pts[i[2]]);
            let area = (bb[0] - a[0]) * (cc[1] - a[1]) - (cc[0] - a[0]) * (bb[1] - a[1]);
            if area.abs() < 1e-9 {
                continue;
            }
            let minx = a[0].min(bb[0]).min(cc[0]).floor().max(0.0) as u32;
            let maxx = a[0].max(bb[0]).max(cc[0]).ceil().min(s - 1.0) as u32;
            let miny = a[1].min(bb[1]).min(cc[1]).floor().max(0.0) as u32;
            let maxy = a[1].max(bb[1]).max(cc[1]).ceil().min(s - 1.0) as u32;
            for y in miny..=maxy {
                for x in minx..=maxx {
                    let p = [x as f32 + 0.5, y as f32 + 0.5];
                    let e = |u: [f32; 3], v: [f32; 3]| (v[0] - u[0]) * (p[1] - u[1]) - (v[1] - u[1]) * (p[0] - u[0]);
                    let w0 = e(bb, cc) / area;
                    let w1 = e(cc, a) / area;
                    let w2 = e(a, bb) / area;
                    if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                        continue;
                    }
                    let z = a[2] * w0 + bb[2] * w1 + cc[2] * w2;
                    let slot = (y * size + x) as usize;
                    if z >= depth[slot] {
                        continue;
                    }
                    let uv = [
                        part.uvs[i[0]][0] * w0 + part.uvs[i[1]][0] * w1 + part.uvs[i[2]][0] * w2,
                        part.uvs[i[0]][1] * w0 + part.uvs[i[1]][1] * w1 + part.uvs[i[2]][1] * w2,
                    ];
                    let col = part.texture.map_or([170.0, 160.0, 150.0, 255.0], |t| sample(t, uv));
                    if part.alpha_test && col[3] < 128.0 {
                        continue;
                    }
                    let n = geom::norm(mirror(geom::add(
                        geom::add(geom::scale(part.normals[i[0]], w0), geom::scale(part.normals[i[1]], w1)),
                        geom::scale(part.normals[i[2]], w2),
                    )));
                    // Two-sided: light the side facing the camera.
                    let n = if geom::dot(n, fwd) > 0.0 { geom::scale(n, -1.0) } else { n };
                    let l = 0.45 + 0.75 * geom::dot(n, light).max(0.0);
                    depth[slot] = z;
                    img.put_pixel(
                        x,
                        y,
                        Rgba([
                            (col[0] * l).min(255.0) as u8,
                            (col[1] * l).min(255.0) as u8,
                            (col[2] * l).min(255.0) as u8,
                            255,
                        ]),
                    );
                }
            }
        }
    }
    img
}

/// Front / right / back / left views side by side (each `size` square).
pub fn turntable(parts: &[Part], size: u32, elevation: f32, offset: f32, background: [u8; 3]) -> Vec<RgbaImage> {
    [0.0, 90.0, 180.0, 270.0]
        .iter()
        .map(|a| view(parts, size, a + offset, elevation, background))
        .collect()
}

/// Foliage sprite of glTF-space primitives (Y up): orthographic side view along `azimuth`
/// (0 = looking along -Z) tilted down by `elevation`, transparent background, alpha-tested
/// texels, soft sky lighting (albedo kept close to its scanned value), 2x supersampled. The
/// model fills the `w` x `h` frame (aspect kept), standing on the bottom edge.
pub fn sprite(
    model: &crate::glb::Model,
    w: u32,
    h: u32,
    azimuth_deg: f32,
    elevation_deg: f32,
) -> RgbaImage {
    let ss = 2u32;
    let (sa, ca) = azimuth_deg.to_radians().sin_cos();
    let (se, ce) = elevation_deg.to_radians().sin_cos();
    let fwd = [-sa * ce, -se, -ca * ce];
    let right = geom::norm(geom::cross(fwd, [0.0, 1.0, 0.0]));
    let up = geom::cross(right, fwd);
    let pts: Vec<V3> = model.primitives.iter().flat_map(|p| p.positions.iter().copied()).collect();
    let (mut minx, mut maxx, mut miny, mut maxy) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for p in &pts {
        let x = geom::dot(*p, right);
        let y = geom::dot(*p, up);
        minx = minx.min(x);
        maxx = maxx.max(x);
        miny = miny.min(y);
        maxy = maxy.max(y);
    }
    let (fw, fh) = ((w * ss) as f32, (h * ss) as f32);
    let k = ((fw * 0.96) / (maxx - minx).max(1e-6)).min((fh * 0.97) / (maxy - miny).max(1e-6));
    let ox = (fw - (maxx - minx) * k) * 0.5;
    let project = |p: V3| {
        [
            ox + (geom::dot(p, right) - minx) * k,
            fh - (geom::dot(p, up) - miny) * k,
            geom::dot(p, fwd),
        ]
    };
    let mut img = RgbaImage::from_pixel(w * ss, h * ss, Rgba([0, 0, 0, 0]));
    let mut depth = vec![f32::INFINITY; (w * ss * h * ss) as usize];
    for prim in &model.primitives {
        let mat = &model.materials[prim.material];
        let pp: Vec<[f32; 3]> = prim.positions.iter().map(|p| project(*p)).collect();
        for t in prim.indices.chunks_exact(3) {
            let i = [t[0] as usize, t[1] as usize, t[2] as usize];
            let (a, b, c) = (pp[i[0]], pp[i[1]], pp[i[2]]);
            let area = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
            if area.abs() < 1e-9 {
                continue;
            }
            let x0 = a[0].min(b[0]).min(c[0]).floor().max(0.0) as u32;
            let x1 = a[0].max(b[0]).max(c[0]).ceil().min(fw - 1.0) as u32;
            let y0 = a[1].min(b[1]).min(c[1]).floor().max(0.0) as u32;
            let y1 = a[1].max(b[1]).max(c[1]).ceil().min(fh - 1.0) as u32;
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let p = [x as f32 + 0.5, y as f32 + 0.5];
                    let e = |u: [f32; 3], v: [f32; 3]| (v[0] - u[0]) * (p[1] - u[1]) - (v[1] - u[1]) * (p[0] - u[0]);
                    let (w0, w1, w2) = (e(b, c) / area, e(c, a) / area, e(a, b) / area);
                    if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                        continue;
                    }
                    let z = a[2] * w0 + b[2] * w1 + c[2] * w2;
                    let slot = (y * w * ss + x) as usize;
                    if z >= depth[slot] {
                        continue;
                    }
                    let uv = [
                        prim.uvs[i[0]][0] * w0 + prim.uvs[i[1]][0] * w1 + prim.uvs[i[2]][0] * w2,
                        prim.uvs[i[0]][1] * w0 + prim.uvs[i[1]][1] * w1 + prim.uvs[i[2]][1] * w2,
                    ];
                    let col = match &mat.base_color {
                        Some(t) => {
                            let s = sample(t, uv);
                            [s[0] * mat.base_factor[0], s[1] * mat.base_factor[1], s[2] * mat.base_factor[2], s[3]]
                        }
                        None => mat.base_factor.map(|f| f * 255.0),
                    };
                    if col[3] < 128.0 {
                        continue;
                    }
                    let n = geom::norm(geom::add(
                        geom::add(geom::scale(prim.normals[i[0]], w0), geom::scale(prim.normals[i[1]], w1)),
                        geom::scale(prim.normals[i[2]], w2),
                    ));
                    let l = 0.82 + 0.18 * n[1].abs();
                    depth[slot] = z;
                    img.put_pixel(
                        x,
                        y,
                        Rgba([
                            (col[0] * l).min(255.0) as u8,
                            (col[1] * l).min(255.0) as u8,
                            (col[2] * l).min(255.0) as u8,
                            255,
                        ]),
                    );
                }
            }
        }
    }
    image::imageops::resize(&img, w, h, image::imageops::FilterType::Triangle)
}
