//! Local remaster profile: replacement meshes, actor parts and textures that override original
//! assets by name. Nothing here is original data; the files are produced by
//! `tools/gothic-assets` into a directory outside the repository and selected at runtime with
//! `GR_REMASTER=<dir>`. Without a profile (or without an entry for a name) every loader keeps
//! the original asset.
//!
//! Profile layout:
//!
//! ```text
//! <dir>/manifest.txt      one entry per line: `<kind> <original name> <relative file>`
//!                         kinds: mesh (`.xcmsh` / `.xlmsh` name), actor (`.xact` stem),
//!                         texture (image stem without `.ximg`); `#` starts a comment
//! <dir>/*.grmo            replacement geometry (this module's binary format)
//! <dir>/*.dds             DXT1 / DXT5 textures with their mip chain
//! ```
//!
//! `.grmo` (little endian): `GRMO`, version u32 = 1, flags u32 (bit 0 skinned), node-name
//! count u32 + names, submesh count u32, submeshes, collision triangle count u32 + 9 f32 each.
//! A submesh: material name, diffuse stem, normal stem (u16 length + UTF-8 each), roughness
//! f32, blend mode u32 (0 opaque, 1 alpha test), vertex count u32, LOD count u32 (>= 1) with
//! the index count of each LOD, positions 3 f32, normals 3 f32, uvs 2 f32, tangents 4 f32
//! (xyz + handedness), and when skinned joints 4 u16 (into the node names) and weights 4 f32,
//! then the indices of every LOD (u32). Geometry is in Gothic space: centimetres, Y up,
//! left-handed, clockwise front faces, exactly like `.xcmsh` / `.xact` vertices.
use anyhow::{Context, Result, bail, ensure};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::OnceLock,
};

const MAGIC: &[u8; 4] = b"GRMO";
const VERSION: u32 = 1;
const MAX_FILE: usize = 256 * 1024 * 1024;
const MAX_VERTICES: usize = 4_000_000;
const MAX_INDICES: usize = 24_000_000;
const MAX_SUBMESHES: usize = 256;
const MAX_NODES: usize = 4096;
const MAX_LODS: usize = 8;
const MAX_NAME: usize = 1024;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct OverrideSubmesh {
    /// Original material to inherit (static: `.xshmat` name; actor: material name in the actor).
    pub material: String,
    /// Diffuse / normal texture stems resolved through the profile first (empty = none).
    pub diffuse: String,
    pub normal: String,
    pub roughness: f32,
    /// 0 opaque, 1 alpha tested.
    pub blend: u32,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub tangents: Vec<[f32; 4]>,
    /// Skinned only: indices into [`OverrideMesh::nodes`] and normalised weights.
    pub joints: Vec<[u16; 4]>,
    pub weights: Vec<[f32; 4]>,
    /// LOD 0 first, then coarser levels.
    pub lods: Vec<Vec<u32>>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct OverrideMesh {
    pub skinned: bool,
    /// Skeleton node names the joints refer to (skinned only).
    pub nodes: Vec<String>,
    pub submeshes: Vec<OverrideSubmesh>,
    /// Simplified collision triangles in the same space.
    pub collision: Vec<[[f32; 3]; 3]>,
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.at.checked_add(n).context("offset overflow")?;
        let s = self.bytes.get(self.at..end).context("truncated GRMO")?;
        self.at = end;
        Ok(s)
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into()?))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }
    fn f32(&mut self) -> Result<f32> {
        let v = f32::from_le_bytes(self.take(4)?.try_into()?);
        ensure!(v.is_finite(), "non-finite value in GRMO");
        Ok(v)
    }
    fn count(&mut self, max: usize, what: &str) -> Result<usize> {
        let n = self.u32()? as usize;
        ensure!(n <= max, "{what} count {n} exceeds {max}");
        Ok(n)
    }
    fn string(&mut self) -> Result<String> {
        let n = self.u16()? as usize;
        ensure!(n <= MAX_NAME, "name too long");
        Ok(std::str::from_utf8(self.take(n)?)?.to_string())
    }
    fn floats<const N: usize>(&mut self, n: usize) -> Result<Vec<[f32; N]>> {
        // Checks the whole range first so a hostile count cannot allocate.
        let bytes = n.checked_mul(N * 4).context("size overflow")?;
        ensure!(self.at + bytes <= self.bytes.len(), "truncated GRMO arrays");
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            let mut v = [0f32; N];
            for x in &mut v {
                *x = self.f32()?;
            }
            out.push(v);
        }
        Ok(out)
    }
}

impl OverrideMesh {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        ensure!(bytes.len() <= MAX_FILE, "GRMO exceeds size ceiling");
        let mut r = Reader { bytes, at: 0 };
        ensure!(r.take(4)? == MAGIC, "not a GRMO file");
        let version = r.u32()?;
        ensure!(version == VERSION, "unsupported GRMO version {version}");
        let flags = r.u32()?;
        let skinned = flags & 1 != 0;
        let node_count = r.count(MAX_NODES, "node")?;
        let nodes = (0..node_count)
            .map(|_| r.string())
            .collect::<Result<Vec<_>>>()?;
        let sub_count = r.count(MAX_SUBMESHES, "submesh")?;
        let mut submeshes = Vec::with_capacity(sub_count);
        let mut total_v = 0usize;
        let mut total_i = 0usize;
        for _ in 0..sub_count {
            let material = r.string()?;
            let diffuse = r.string()?;
            let normal = r.string()?;
            let roughness = r.f32()?;
            let blend = r.u32()?;
            let n = r.count(MAX_VERTICES, "vertex")?;
            total_v += n;
            ensure!(total_v <= MAX_VERTICES, "vertex budget exceeded");
            let lod_count = r.count(MAX_LODS, "LOD")?;
            ensure!(lod_count >= 1, "submesh without LOD 0");
            let lod_len = (0..lod_count)
                .map(|_| r.count(MAX_INDICES, "index"))
                .collect::<Result<Vec<_>>>()?;
            let positions = r.floats::<3>(n)?;
            let normals = r.floats::<3>(n)?;
            let uvs = r.floats::<2>(n)?;
            let tangents = r.floats::<4>(n)?;
            let (mut joints, mut weights) = (Vec::new(), Vec::new());
            if skinned {
                ensure!(r.at + n * 8 <= bytes.len(), "truncated joints");
                for _ in 0..n {
                    let j = [r.u16()?, r.u16()?, r.u16()?, r.u16()?];
                    ensure!(
                        j.iter().all(|&j| (j as usize) < node_count.max(1)),
                        "joint index out of range"
                    );
                    joints.push(j);
                }
                weights = r.floats::<4>(n)?;
            }
            let mut lods = Vec::with_capacity(lod_count);
            for len in lod_len {
                total_i += len;
                ensure!(total_i <= MAX_INDICES, "index budget exceeded");
                ensure!(len % 3 == 0, "index count not a multiple of 3");
                ensure!(r.at + len * 4 <= bytes.len(), "truncated indices");
                let mut idx = Vec::with_capacity(len);
                for _ in 0..len {
                    let i = r.u32()?;
                    ensure!((i as usize) < n, "index out of range");
                    idx.push(i);
                }
                lods.push(idx);
            }
            submeshes.push(OverrideSubmesh {
                material,
                diffuse,
                normal,
                roughness,
                blend,
                positions,
                normals,
                uvs,
                tangents,
                joints,
                weights,
                lods,
            });
        }
        let tri = r.count(MAX_INDICES / 3, "collision triangle")?;
        let flat = r.floats::<9>(tri)?;
        let collision = flat
            .into_iter()
            .map(|t| [[t[0], t[1], t[2]], [t[3], t[4], t[5]], [t[6], t[7], t[8]]])
            .collect();
        ensure!(r.at == bytes.len(), "trailing bytes in GRMO");
        Ok(Self {
            skinned,
            nodes,
            submeshes,
            collision,
        })
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut o = Vec::new();
        let s = |o: &mut Vec<u8>, v: &str| -> Result<()> {
            ensure!(v.len() <= MAX_NAME, "name too long");
            o.extend((v.len() as u16).to_le_bytes());
            o.extend(v.as_bytes());
            Ok(())
        };
        let f = |o: &mut Vec<u8>, v: &[f32]| v.iter().for_each(|x| o.extend(x.to_le_bytes()));
        o.extend(MAGIC);
        o.extend(VERSION.to_le_bytes());
        o.extend(u32::from(self.skinned).to_le_bytes());
        o.extend((self.nodes.len() as u32).to_le_bytes());
        for n in &self.nodes {
            s(&mut o, n)?;
        }
        o.extend((self.submeshes.len() as u32).to_le_bytes());
        for sub in &self.submeshes {
            let n = sub.positions.len();
            ensure!(
                sub.normals.len() == n && sub.uvs.len() == n && sub.tangents.len() == n,
                "vertex streams differ in length"
            );
            ensure!(
                !self.skinned || (sub.joints.len() == n && sub.weights.len() == n),
                "skin streams differ in length"
            );
            ensure!(!sub.lods.is_empty(), "submesh without LOD 0");
            s(&mut o, &sub.material)?;
            s(&mut o, &sub.diffuse)?;
            s(&mut o, &sub.normal)?;
            f(&mut o, &[sub.roughness]);
            o.extend(sub.blend.to_le_bytes());
            o.extend((n as u32).to_le_bytes());
            o.extend((sub.lods.len() as u32).to_le_bytes());
            for l in &sub.lods {
                o.extend((l.len() as u32).to_le_bytes());
            }
            sub.positions.iter().for_each(|v| f(&mut o, v));
            sub.normals.iter().for_each(|v| f(&mut o, v));
            sub.uvs.iter().for_each(|v| f(&mut o, v));
            sub.tangents.iter().for_each(|v| f(&mut o, v));
            if self.skinned {
                for j in &sub.joints {
                    j.iter().for_each(|x| o.extend(x.to_le_bytes()));
                }
                sub.weights.iter().for_each(|v| f(&mut o, v));
            }
            for l in &sub.lods {
                l.iter().for_each(|i| o.extend(i.to_le_bytes()));
            }
        }
        o.extend((self.collision.len() as u32).to_le_bytes());
        for t in &self.collision {
            t.iter().for_each(|v| f(&mut o, v));
        }
        Ok(o)
    }
}

/// A DXT1 / DXT5 texture with its mip chain (largest first).
#[derive(Debug, Clone, PartialEq)]
pub struct DdsTexture {
    pub width: u32,
    pub height: u32,
    pub fourcc: [u8; 4],
    pub mips: Vec<Vec<u8>>,
}

fn mip_bytes(fourcc: &[u8; 4], w: u32, h: u32) -> usize {
    let block = if fourcc == b"DXT1" { 8 } else { 16 };
    (w.div_ceil(4).max(1) * h.div_ceil(4).max(1)) as usize * block
}

impl DdsTexture {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        ensure!(bytes.len() <= MAX_FILE, "DDS exceeds size ceiling");
        ensure!(bytes.get(..4) == Some(b"DDS "), "not a DDS file");
        ensure!(bytes.len() >= 128, "truncated DDS header");
        let word = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().expect("4 bytes"));
        let height = word(12);
        let width = word(16);
        ensure!(
            (4..=16384).contains(&width) && (4..=16384).contains(&height),
            "unsupported DDS dimensions"
        );
        let mip_count = word(28).max(1);
        ensure!(mip_count <= 15, "invalid mip count");
        let fourcc: [u8; 4] = bytes[84..88].try_into()?;
        if !matches!(&fourcc, b"DXT1" | b"DXT5") {
            bail!("unsupported DDS format {fourcc:?}");
        }
        let mut at = 128usize;
        let mut mips = Vec::new();
        let (mut w, mut h) = (width, height);
        for _ in 0..mip_count {
            let n = mip_bytes(&fourcc, w, h);
            let data = bytes.get(at..at + n).context("truncated DDS mip")?;
            mips.push(data.to_vec());
            at += n;
            if w == 1 && h == 1 {
                break;
            }
            w = (w / 2).max(1);
            h = (h / 2).max(1);
        }
        Ok(Self {
            width,
            height,
            fourcc,
            mips,
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut h = vec![0u8; 128];
        let mut put = |o: usize, v: u32| h[o..o + 4].copy_from_slice(&v.to_le_bytes());
        put(4, 124);
        // CAPS | HEIGHT | WIDTH | PIXELFORMAT | MIPMAPCOUNT | LINEARSIZE
        put(8, 0x1 | 0x2 | 0x4 | 0x1000 | 0x20000 | 0x80000);
        put(12, self.height);
        put(16, self.width);
        put(20, self.mips.first().map_or(0, |m| m.len() as u32));
        put(28, self.mips.len() as u32);
        put(76, 32);
        put(80, 0x4); // DDPF_FOURCC
        put(108, 0x1000 | 0x8 | 0x400000); // TEXTURE | COMPLEX | MIPMAP
        h[0..4].copy_from_slice(b"DDS ");
        h[84..88].copy_from_slice(&self.fourcc);
        for m in &self.mips {
            h.extend(m);
        }
        h
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OverrideKind {
    Mesh,
    Actor,
    Texture,
}

/// A loaded profile manifest; files are read on demand.
#[derive(Debug, Default)]
pub struct Profile {
    pub root: PathBuf,
    entries: HashMap<(OverrideKind, String), PathBuf>,
}

/// Lower-case basename without the extension: the key every original name is matched by.
pub fn key_of(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let stem = match base.rsplit_once('.') {
        Some((s, ext))
            if matches!(
                ext.to_ascii_lowercase().as_str(),
                "xcmsh" | "xlmsh" | "xact" | "ximg" | "fxa"
            ) =>
        {
            s
        }
        _ => base,
    };
    stem.to_ascii_lowercase()
}

impl Profile {
    pub fn open(root: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(root.join("manifest.txt"))
            .with_context(|| format!("reading {}/manifest.txt", root.display()))?;
        let mut entries = HashMap::new();
        for (n, line) in text.lines().enumerate() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let mut it = line.split_whitespace();
            let (Some(kind), Some(name), Some(file), None) =
                (it.next(), it.next(), it.next(), it.next())
            else {
                bail!("manifest line {}: expected `<kind> <name> <file>`", n + 1);
            };
            let kind = match kind {
                "mesh" => OverrideKind::Mesh,
                "actor" => OverrideKind::Actor,
                "texture" => OverrideKind::Texture,
                other => bail!("manifest line {}: unknown kind {other}", n + 1),
            };
            ensure!(
                !file.contains("..") && !Path::new(file).is_absolute(),
                "manifest line {}: file must stay inside the profile",
                n + 1
            );
            entries.insert((kind, key_of(name)), root.join(file));
        }
        Ok(Self {
            root: root.to_path_buf(),
            entries,
        })
    }

    /// The profile named by `GR_REMASTER`, opened once; `None` when unset or unreadable.
    pub fn global() -> Option<&'static Profile> {
        static P: OnceLock<Option<Profile>> = OnceLock::new();
        P.get_or_init(|| {
            let dir = std::env::var_os("GR_REMASTER")?;
            match Profile::open(Path::new(&dir)) {
                Ok(p) => {
                    eprintln!(
                        "remaster profile {}: {} overrides",
                        p.root.display(),
                        p.entries.len()
                    );
                    Some(p)
                }
                Err(e) => {
                    eprintln!("remaster profile ignored: {e:#}");
                    None
                }
            }
        })
        .as_ref()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn path(&self, kind: OverrideKind, name: &str) -> Option<&Path> {
        self.entries
            .get(&(kind, key_of(name)))
            .map(PathBuf::as_path)
    }

    fn read(&self, kind: OverrideKind, name: &str) -> Option<Vec<u8>> {
        let path = self.path(kind, name)?;
        match std::fs::read(path) {
            Ok(b) => Some(b),
            Err(e) => {
                eprintln!("remaster {}: {e}", path.display());
                None
            }
        }
    }

    /// Replacement geometry for a static mesh or actor; errors are logged and fall back.
    pub fn mesh(&self, kind: OverrideKind, name: &str) -> Option<OverrideMesh> {
        let bytes = self.read(kind, name)?;
        OverrideMesh::parse(&bytes)
            .map_err(|e| eprintln!("remaster {name}: {e:#}"))
            .ok()
    }

    pub fn texture(&self, stem: &str) -> Option<DdsTexture> {
        let bytes = self.read(OverrideKind::Texture, stem)?;
        DdsTexture::parse(&bytes)
            .map_err(|e| eprintln!("remaster texture {stem}: {e:#}"))
            .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(skinned: bool) -> OverrideMesh {
        OverrideMesh {
            skinned,
            nodes: if skinned {
                vec!["Bip01".into(), "Bip01 Head".into()]
            } else {
                vec![]
            },
            submeshes: vec![OverrideSubmesh {
                material: "Synthetic_A.xshmat".into(),
                diffuse: "synthetic_d".into(),
                normal: "synthetic_n".into(),
                roughness: 0.7,
                blend: 0,
                positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                normals: vec![[0.0, 0.0, 1.0]; 3],
                uvs: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
                tangents: vec![[1.0, 0.0, 0.0, 1.0]; 3],
                joints: if skinned {
                    vec![[0, 1, 0, 0]; 3]
                } else {
                    vec![]
                },
                weights: if skinned {
                    vec![[0.5, 0.5, 0.0, 0.0]; 3]
                } else {
                    vec![]
                },
                lods: vec![vec![0, 1, 2], vec![0, 1, 2]],
            }],
            collision: vec![[[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]],
        }
    }

    #[test]
    fn grmo_round_trips_and_rejects_truncation() -> Result<()> {
        for skinned in [false, true] {
            let m = sample(skinned);
            let bytes = m.to_bytes()?;
            assert_eq!(OverrideMesh::parse(&bytes)?, m);
            for cut in 0..bytes.len() {
                assert!(OverrideMesh::parse(&bytes[..cut]).is_err(), "cut {cut}");
            }
        }
        Ok(())
    }

    #[test]
    fn grmo_rejects_out_of_range_index() -> Result<()> {
        let mut m = sample(false);
        m.submeshes[0].lods[0][2] = 9;
        assert!(OverrideMesh::parse(&m.to_bytes()?).is_err());
        Ok(())
    }

    #[test]
    fn dds_round_trips() -> Result<()> {
        let t = DdsTexture {
            width: 8,
            height: 4,
            fourcc: *b"DXT5",
            mips: vec![vec![7; 32], vec![1; 16], vec![2; 16], vec![3; 16]],
        };
        assert_eq!(DdsTexture::parse(&t.to_bytes())?, t);
        assert!(DdsTexture::parse(&t.to_bytes()[..140]).is_err());
        Ok(())
    }

    #[test]
    fn manifest_keys_ignore_case_and_extension() -> Result<()> {
        let dir = std::env::temp_dir().join(format!("grmo-manifest-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        std::fs::write(
            dir.join("manifest.txt"),
            "# synthetic\nmesh Synthetic_Barrel.xcmsh barrel.grmo\ntexture Syn_D syn_d.dds\n",
        )?;
        let p = Profile::open(&dir)?;
        assert!(
            p.path(OverrideKind::Mesh, "dir/SYNTHETIC_barrel.XCMSH")
                .is_some()
        );
        assert!(p.path(OverrideKind::Texture, "syn_d.ximg").is_some());
        assert!(p.path(OverrideKind::Actor, "Synthetic_Barrel").is_none());
        std::fs::write(dir.join("manifest.txt"), "mesh a ../escape.grmo\n")?;
        assert!(Profile::open(&dir).is_err());
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }
}
