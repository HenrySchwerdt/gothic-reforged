//! `eCResourceLightmap_PS` (`.xlmp`, `Lightmaps.pak`): baked static-light data of one placed mesh
//! instance. Layout and open questions: `docs/research/LIGHTMAP_FORMAT.md`. Bounded; the per-vertex
//! arrays and the tile images are decoded, the mapping of tiles to triangles is NOT.
use crate::genome::GenomeFile;
use anyhow::{Context, Result, bail, ensure};

const MAX_VERTICES: usize = 4_000_000;
const MAX_BLOCKS: usize = 4096;
const MAX_TILE_EDGE: usize = 4096;
const TILE_MARKER: [u8; 4] = [0x44, 0x33, 0x22, 0x11];

/// One baked tile: BGRA texels (empty when the exporter stored no data, i.e. no light) and the
/// per-row valid span `[start, end)` of the packed chart (`h` entries when data is present).
#[derive(Debug, Clone, PartialEq)]
pub struct LightmapTile {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
    pub spans: Vec<[u16; 2]>,
}

/// Data of one submesh, in the submesh's vertex order.
#[derive(Debug, Clone, PartialEq)]
pub struct LightmapBlock {
    /// Per vertex `[b, g, r, a]`: colour of the static lights reaching the vertex (rgb, mostly 0)
    /// and baked ambient occlusion in `a` (equals the mesh's own tag-5 vertex colour alpha).
    pub light: Vec<[u8; 4]>,
    /// Per vertex unit vector (dominant incident light direction, `(1,0,0)` when unlit).
    pub dir: Vec<[f32; 3]>,
    /// Tiles of the lightmap charts (`uv2` of the mesh addresses them in `1/texel_scale` units).
    pub tiles: Vec<LightmapTile>,
}

impl LightmapBlock {
    /// Light colour of vertex `k` in 0..1 (`None` out of range).
    pub fn light_rgb(&self, k: usize) -> Option<[f32; 3]> {
        let c = self.light.get(k)?;
        Some([c[2], c[1], c[0]].map(|v| f32::from(v) / 255.0))
    }
    /// True when any vertex receives baked light colour.
    pub fn has_vertex_light(&self) -> bool {
        self.light.iter().any(|c| c[0] | c[1] | c[2] != 0)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Xlmp {
    /// Render mesh the data was baked for (string table, as serialized).
    pub mesh: String,
    /// Texels per `uv2` unit (0.04: one texel = 25 cm).
    pub texel_scale: f32,
    pub blocks: Vec<LightmapBlock>,
}

struct Cur<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Cur<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.p.checked_add(n).context("offset overflow")?;
        let s = self.b.get(self.p..end).context("truncated xlmp")?;
        self.p = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into()?))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }
    fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_bits(self.u32()?))
    }
    fn remaining(&self) -> usize {
        self.b.len() - self.p
    }
}

impl Xlmp {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        Self::parse_opts(bytes, true)
    }

    /// Like [`Xlmp::parse`]; with `keep_tiles == false` the tile images are validated and skipped
    /// (the returned blocks have no tiles), which avoids copying megabytes of texels.
    pub fn parse_opts(bytes: &[u8], keep_tiles: bool) -> Result<Self> {
        let genome = GenomeFile::parse(bytes)?;
        ensure!(
            genome
                .strings
                .iter()
                .any(|s| *s == b"eCResourceLightmap_PS"),
            "not an eCResourceLightmap_PS resource"
        );
        let mesh = genome
            .strings
            .iter()
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .find(|s| {
                let l = s.to_ascii_lowercase();
                l.ends_with(".xcmsh") || l.ends_with(".xlmsh")
            })
            .context("xlmp names no mesh resource")?;
        let body = &bytes[..genome.table_offset];
        // First block header `01 03 00`, preceded by `f32 texel_scale, u32 block_count`.
        let first = body
            .windows(8)
            .position(|w| w[..3] == [1, 3, 0] && w[3] == 1 && w[4..8] != [0, 0, 0, 0])
            .context("no lightmap block found")?;
        ensure!(first >= 22, "block header too close to start");
        let mut c = Cur {
            b: body,
            p: first - 8,
        };
        let texel_scale = c.f32()?;
        ensure!(
            texel_scale.is_finite() && texel_scale > 0.0 && texel_scale < 100.0,
            "implausible texel scale"
        );
        let count = c.u32()? as usize;
        ensure!((1..=MAX_BLOCKS).contains(&count), "implausible block count");
        let mut blocks = Vec::with_capacity(count.min(64));
        for _ in 0..count {
            blocks.push(parse_block(&mut c, keep_tiles)?);
        }
        Ok(Self {
            mesh,
            texel_scale,
            blocks,
        })
    }
}

fn array_count(c: &mut Cur, elem: usize) -> Result<usize> {
    ensure!(c.u8()? == 1, "unexpected array tag");
    let n = c.u32()? as usize;
    ensure!(n <= MAX_VERTICES, "vertex count exceeds ceiling");
    ensure!(
        n.checked_mul(elem).is_some_and(|b| b <= c.remaining()),
        "array exceeds data"
    );
    Ok(n)
}

fn parse_block(c: &mut Cur, keep_tiles: bool) -> Result<LightmapBlock> {
    ensure!(c.take(3)? == [1, 3, 0], "unexpected block header");
    let n = array_count(c, 4)?;
    let light: Vec<[u8; 4]> = c.take(n * 4)?.as_chunks::<4>().0.to_vec();
    let m = array_count(c, 12)?;
    ensure!(m == n, "direction array length differs from vertex array");
    let dir = c
        .take(m * 12)?
        .as_chunks::<12>()
        .0
        .iter()
        .map(|v| [0, 4, 8].map(|o| f32::from_le_bytes([v[o], v[o + 1], v[o + 2], v[o + 3]])))
        .collect::<Vec<_>>();
    ensure!(
        dir.iter().flatten().all(|f| f.is_finite()),
        "non-finite direction"
    );
    let tile_count = c.u16()? as usize;
    let mut tiles = Vec::with_capacity(tile_count.min(1024));
    for i in 0..tile_count {
        ensure!(c.take(4)? == TILE_MARKER, "missing tile marker");
        ensure!(c.u16()? == 1, "unsupported tile version");
        ensure!(c.u32()? as usize == i, "tile index out of sequence");
        c.take(8)?;
        let w = c.u32()? as usize;
        let h = c.u32()? as usize;
        let size = c.u32()? as usize;
        ensure!(
            (1..=MAX_TILE_EDGE).contains(&w) && (1..=MAX_TILE_EDGE).contains(&h),
            "implausible tile size"
        );
        if size == 0 {
            if keep_tiles {
                tiles.push(LightmapTile {
                    width: w as u32,
                    height: h as u32,
                    bgra: Vec::new(),
                    spans: Vec::new(),
                });
            }
            continue;
        }
        if size != w * h * 4 {
            bail!("tile pixel size {size} differs from {w}x{h} BGRA");
        }
        let texels = c.take(size)?;
        let n = c.u32()? as usize;
        ensure!(n == h * 4, "tile span table size differs from height");
        let span_bytes = c.take(n)?;
        if !keep_tiles {
            continue;
        }
        let bgra = texels.to_vec();
        let spans = span_bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|s| {
                [
                    u16::from_le_bytes([s[0], s[1]]),
                    u16::from_le_bytes([s[2], s[3]]),
                ]
            })
            .collect();
        tiles.push(LightmapTile {
            width: w as u32,
            height: h as u32,
            bgra,
            spans,
        });
    }
    Ok(LightmapBlock { light, dir, tiles })
}

/// Archive member name of the lightmap of `mesh_stem` placed as entity `source_id` (first 16 bytes
/// of the 20-byte source ID, stored in Microsoft mixed-endian GUID order).
pub fn lightmap_file_name(mesh_stem: &str, source_id: &[u8]) -> Option<String> {
    let g = source_id.get(..16)?;
    Some(format!(
        "{mesh_stem}_{{{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}}}.xlmp",
        g[3],
        g[2],
        g[1],
        g[0],
        g[5],
        g[4],
        g[7],
        g[6],
        g[8],
        g[9],
        g[10],
        g[11],
        g[12],
        g[13],
        g[14],
        g[15]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(n: usize, tiles: &[(u32, u32, bool)], light: &[u8]) -> Vec<u8> {
        let mut o = vec![1, 3, 0, 1];
        o.extend((n as u32).to_le_bytes());
        for i in 0..n {
            o.extend_from_slice(&light[(i * 4) % light.len()..][..4]);
        }
        o.push(1);
        o.extend((n as u32).to_le_bytes());
        for _ in 0..n {
            for f in [1.0f32, 0.0, 0.0] {
                o.extend(f.to_le_bytes());
            }
        }
        o.extend((tiles.len() as u16).to_le_bytes());
        for (i, (w, h, data)) in tiles.iter().enumerate() {
            o.extend(TILE_MARKER);
            o.extend(1u16.to_le_bytes());
            o.extend((i as u32).to_le_bytes());
            o.extend([0xff; 8]);
            o.extend(w.to_le_bytes());
            o.extend(h.to_le_bytes());
            if *data {
                o.extend((w * h * 4).to_le_bytes());
                o.extend((0..w * h * 4).map(|v| v as u8));
                o.extend((h * 4).to_le_bytes());
                for _ in 0..*h {
                    o.extend(0u16.to_le_bytes());
                    o.extend((*w as u16).to_le_bytes());
                }
            } else {
                o.extend(0u32.to_le_bytes());
            }
        }
        o
    }

    fn file(blocks: &[Vec<u8>]) -> Vec<u8> {
        let mut body = vec![7u8; 60];
        body.extend(0.04f32.to_le_bytes());
        body.extend((blocks.len() as u32).to_le_bytes());
        for b in blocks {
            body.extend(b);
        }
        let mut f = b"GENOMFLE".to_vec();
        f.extend([1, 0]);
        let table = 14 + body.len();
        f.extend((table as u32).to_le_bytes());
        f.extend(body);
        f.extend([0xef, 0xbe, 0xad, 0xde, 1]);
        let strings = [
            "eCResourceLightmap_PS",
            "/Data/_compiledMesh/x/House_01.xcmsh",
        ];
        f.extend((strings.len() as u32).to_le_bytes());
        for s in strings {
            f.extend((s.len() as u16).to_le_bytes());
            f.extend(s.as_bytes());
        }
        f
    }

    #[test]
    fn parses_blocks_vertices_and_tiles() -> Result<()> {
        let b0 = block(
            3,
            &[(2, 2, true), (3, 1, false)],
            &[1, 2, 3, 200, 0, 0, 0, 255, 0, 0, 0, 255],
        );
        let b1 = block(2, &[], &[0, 0, 0, 255]);
        let x = Xlmp::parse(&file(&[b0, b1]))?;
        assert_eq!(x.mesh, "/Data/_compiledMesh/x/House_01.xcmsh");
        assert_eq!(x.blocks.len(), 2);
        assert_eq!(x.blocks[0].light[0], [1, 2, 3, 200]);
        assert_eq!(
            x.blocks[0].light_rgb(0),
            Some([3.0 / 255.0, 2.0 / 255.0, 1.0 / 255.0])
        );
        assert!(x.blocks[0].has_vertex_light() && !x.blocks[1].has_vertex_light());
        assert_eq!(x.blocks[0].tiles.len(), 2);
        assert_eq!(x.blocks[0].tiles[0].bgra.len(), 16);
        assert_eq!(x.blocks[0].tiles[0].spans, vec![[0, 2], [0, 2]]);
        assert!(x.blocks[0].tiles[1].bgra.is_empty());
        assert_eq!(x.blocks[1].dir.len(), 2);
        Ok(())
    }

    #[test]
    fn rejects_corruption() {
        let good = file(&[block(3, &[(2, 2, true)], &[0, 0, 0, 255])]);
        assert!(Xlmp::parse(&good).is_ok());
        let table = u32::from_le_bytes(good[10..14].try_into().unwrap()) as usize;
        // Truncate inside the tile table: move the string table up and fix the offset.
        for cut in [table - 3, table - 40] {
            let mut t = good[..cut].to_vec();
            t.extend_from_slice(&good[table..]);
            t[10..14].copy_from_slice(&(cut as u32).to_le_bytes());
            assert!(Xlmp::parse(&t).is_err());
        }
        let mut bad = good.clone();
        let p = bad.windows(4).position(|w| w == TILE_MARKER).unwrap();
        bad[p + 6] = 9;
        assert!(Xlmp::parse(&bad).is_err());
        assert!(Xlmp::parse(b"GENOMFLE").is_err());
    }

    #[test]
    fn guid_file_name_is_mixed_endian() {
        let id = [
            0x52, 0xe7, 0xb0, 0xcb, 0x07, 0x31, 0xf8, 0x4a, 0xb5, 0xeb, 0xb7, 0x70, 0x35, 0xc4,
            0x04, 0x74, 0, 0, 0, 0,
        ];
        assert_eq!(
            lightmap_file_name("G3_Bridge", &id).as_deref(),
            Some("G3_Bridge_{cbb0e752-3107-4af8-b5eb-b77035c40474}.xlmp")
        );
        assert!(lightmap_file_name("x", &id[..8]).is_none());
    }
}
