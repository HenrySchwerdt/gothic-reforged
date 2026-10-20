//! Bounded reader for `g3.igl`, the image group list (`eCImageAdmin::LoadGroupList`).
//!
//! Layout (little endian), see `docs/research/UI_ORIGINAL.md`:
//! `"GENOMFLE"`, `u16 version`, `u32 string_table_offset`, `u32 group_count`, `group_count x u16`
//! group name ids; groups start at `0x2c`. Group: `u32 version(2)`, four `u16` string ids (name,
//! compiled dir, import dir, prefix), `u32 max_w`, `u32 max_h`, nine `u32` of unknown purpose,
//! `u32 tile_count`, tiles of 20 bytes (`u16 image id`, `u16 texture id`, `u32 l, t, r, b`).
//! The string table at `string_table_offset` is `5` bytes of tag, `u32 count`, then
//! `u16 len` + bytes per string. No original content is embedded here.
use anyhow::{Result, bail, ensure};
use std::collections::HashMap;

const MAX_STRINGS: usize = 65_536;
const MAX_TILES: usize = 65_536;
const MAX_GROUPS: usize = 256;
const GROUPS_START: usize = 0x2c;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IglTile {
    /// Source image path (`#G3:\Data\gui\icons\...\IT_Apple.tga`).
    pub image: String,
    /// Compiled atlas path (`Data\gui\icons\compiled\/g3_hud_it_alchemy_00.dds`).
    pub texture: String,
    /// `(left, top, right, bottom)` in pixels, right/bottom exclusive.
    pub rect: [u32; 4],
}

impl IglTile {
    /// Lower-cased bare file name of the source image (the `FindImageIndex` key).
    pub fn key(&self) -> String {
        bare_key(&self.image)
    }

    /// Atlas file stem inside `gui.pak/icons/compiled/` (`g3_hud_it_alchemy_00`).
    pub fn texture_stem(&self) -> String {
        let f = self.texture.rsplit(['\\', '/']).next().unwrap_or("");
        f.strip_suffix(".dds").unwrap_or(f).to_ascii_lowercase()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IglGroup {
    pub name: String,
    pub compiled_dir: String,
    pub import_dir: String,
    pub prefix: String,
    pub max_size: (u32, u32),
    pub tiles: Vec<IglTile>,
}

#[derive(Debug, Clone, Default)]
pub struct Igl {
    pub groups: Vec<IglGroup>,
}

/// Lower-cased bare file name of an image path (either separator).
pub fn bare_key(path: &str) -> String {
    path.rsplit(['\\', '/'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

struct Cur<'a> {
    b: &'a [u8],
    p: usize,
}

impl Cur<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        let end = self.p.checked_add(n).filter(|&e| e <= self.b.len());
        let Some(end) = end else {
            bail!("igl: truncated at {:#x}", self.p)
        };
        let s = &self.b[self.p..end];
        self.p = end;
        Ok(s)
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into()?))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }
}

impl Igl {
    pub fn parse(b: &[u8]) -> Result<Self> {
        ensure!(b.get(..8) == Some(b"GENOMFLE"), "igl: bad magic");
        let mut c = Cur { b, p: 10 };
        let sto = c.u32()? as usize;
        let group_count = c.u32()? as usize;
        ensure!(group_count <= MAX_GROUPS, "igl: too many groups");
        ensure!(
            sto >= GROUPS_START && sto <= b.len(),
            "igl: bad string table"
        );
        let mut s = Cur { b, p: sto + 5 };
        let n = s.u32()? as usize;
        ensure!(n <= MAX_STRINGS, "igl: too many strings");
        let mut strings = Vec::with_capacity(n);
        for _ in 0..n {
            let len = s.u16()? as usize;
            strings.push(s.take(len)?.iter().map(|&c| c as char).collect::<String>());
        }
        let st = |id: u16| -> Result<String> {
            strings
                .get(id as usize)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("igl: string id {id} out of range"))
        };
        let mut c = Cur {
            b: &b[..sto],
            p: GROUPS_START,
        };
        let mut groups = Vec::new();
        while c.p < sto {
            ensure!(groups.len() < MAX_GROUPS, "igl: too many groups");
            ensure!(
                c.u32()? == 2,
                "igl: unknown group version at {:#x}",
                c.p - 4
            );
            let (n0, n1, n2, n3) = (c.u16()?, c.u16()?, c.u16()?, c.u16()?);
            let (mw, mh) = (c.u32()?, c.u32()?);
            c.take(36)?;
            let count = c.u32()? as usize;
            ensure!(count <= MAX_TILES, "igl: too many tiles");
            let mut tiles = Vec::with_capacity(count);
            for _ in 0..count {
                let (i, t) = (c.u16()?, c.u16()?);
                let rect = [c.u32()?, c.u32()?, c.u32()?, c.u32()?];
                tiles.push(IglTile {
                    image: st(i)?,
                    texture: st(t)?,
                    rect,
                });
            }
            groups.push(IglGroup {
                name: st(n0)?,
                compiled_dir: st(n1)?,
                import_dir: st(n2)?,
                prefix: st(n3)?,
                max_size: (mw, mh),
                tiles,
            });
        }
        Ok(Self { groups })
    }

    /// Case-insensitive bare-file-name lookup over all groups
    /// (`eCImageAdmin::FindImageIndex`): first match wins.
    pub fn index(&self) -> HashMap<String, (usize, usize)> {
        let mut m = HashMap::new();
        for (g, grp) in self.groups.iter().enumerate() {
            for (t, tile) in grp.tiles.iter().enumerate() {
                m.entry(tile.key()).or_insert((g, t));
            }
        }
        m
    }

    pub fn group(&self, name: &str) -> Option<&IglGroup> {
        self.groups
            .iter()
            .find(|g| g.name.eq_ignore_ascii_case(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synth() -> Vec<u8> {
        let strings = [
            "g",
            "dir",
            "imp",
            "pfx",
            "#G3:\\a\\Sub\\IT_Apple.tga",
            "t\\/g_00.dds",
        ];
        let mut b = b"GENOMFLE".to_vec();
        b.extend([1, 0]);
        let sto_at = b.len();
        b.extend([0; 4]);
        b.extend(1u32.to_le_bytes());
        b.resize(GROUPS_START, 0);
        b.extend(2u32.to_le_bytes());
        for id in 0..4u16 {
            b.extend(id.to_le_bytes());
        }
        b.extend(1024u32.to_le_bytes());
        b.extend(1024u32.to_le_bytes());
        b.extend([0; 36]);
        b.extend(1u32.to_le_bytes());
        b.extend(4u16.to_le_bytes());
        b.extend(5u16.to_le_bytes());
        for v in [1u32, 1, 65, 65] {
            b.extend(v.to_le_bytes());
        }
        let sto = b.len() as u32;
        b[sto_at..sto_at + 4].copy_from_slice(&sto.to_le_bytes());
        b.extend([0; 5]);
        b.extend((strings.len() as u32).to_le_bytes());
        for s in strings {
            b.extend((s.len() as u16).to_le_bytes());
            b.extend(s.as_bytes());
        }
        b
    }

    #[test]
    fn parses_synthetic() {
        let igl = Igl::parse(&synth()).unwrap();
        assert_eq!(igl.groups.len(), 1);
        let t = &igl.groups[0].tiles[0];
        assert_eq!(t.key(), "it_apple.tga");
        assert_eq!(t.texture_stem(), "g_00");
        assert_eq!(t.rect, [1, 1, 65, 65]);
        assert_eq!(igl.index()["it_apple.tga"], (0, 0));
    }

    #[test]
    fn rejects_truncated() {
        let b = synth();
        for n in [0, 8, 20, 0x30, b.len() - 3] {
            assert!(Igl::parse(&b[..n]).is_err());
        }
        assert!(Igl::parse(b"nope").is_err());
    }
}
