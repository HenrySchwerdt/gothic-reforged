//! Mean colour (from the DXT block endpoints, mip 0) of every image whose name contains a
//! pattern: finds textures that are close to white. Usage: tex_mean PATTERN [MIN_LUMA]
use gothic_formats::{PakArchive, ximg::Ximg};

fn rgb565(v: u16) -> [f32; 3] {
    [
        ((v >> 11) & 31) as f32 / 31.0,
        ((v >> 5) & 63) as f32 / 63.0,
        (v & 31) as f32 / 31.0,
    ]
}

fn main() -> anyhow::Result<()> {
    let dir = r"D:\SteamLibrary\steamapps\common\Gothic 3\Data";
    let mut args = std::env::args().skip(1);
    let pat = args.next().unwrap_or_default().to_lowercase();
    let min: f32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
    // Optional third argument: a directory that receives the matching images as `.dds`.
    let out_dir = args.next();
    let mut img = PakArchive::open(format!(r"{dir}\_compiledImage.pak"))?;
    let entries: Vec<(usize, String)> = img
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| !e.deleted() && e.path.to_lowercase().contains(&pat))
        .map(|(i, e)| (i, e.path.clone()))
        .collect();
    for (i, path) in entries {
        let Ok(bytes) = img.read_payload(i) else { continue };
        let Ok(x) = Ximg::parse(&bytes) else { continue };
        if let Some(d) = &out_dir {
            let name = path.rsplit('/').next().unwrap_or(&path).replace(".ximg", ".dds");
            std::fs::create_dir_all(d)?;
            std::fs::write(format!("{d}/{name}"), x.to_dds())?;
        }
        let Some(m0) = x.mips().first() else { continue };
        let stride = match &x.fourcc {
            b"DXT1" => 8,
            b"DXT3" | b"DXT5" => 16,
            _ => continue,
        };
        let off = if stride == 8 { 0 } else { 8 };
        let (mut sum, mut n) = ([0f32; 3], 0f32);
        for b in m0.chunks_exact(stride) {
            let c0 = rgb565(u16::from_le_bytes([b[off], b[off + 1]]));
            let c1 = rgb565(u16::from_le_bytes([b[off + 2], b[off + 3]]));
            for k in 0..3 {
                sum[k] += (c0[k] + c1[k]) * 0.5;
            }
            n += 1.0;
        }
        if n == 0.0 {
            continue;
        }
        let m = [sum[0] / n, sum[1] / n, sum[2] / n];
        let luma = 0.299 * m[0] + 0.587 * m[1] + 0.114 * m[2];
        if luma >= min {
            println!("{luma:.3} [{:.2} {:.2} {:.2}] {}x{} {}", m[0], m[1], m[2], x.width, x.height, path);
        }
    }
    Ok(())
}
