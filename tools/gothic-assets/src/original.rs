//! Read-only access to the original assets the generated models replace.
use crate::geom::V3;
use anyhow::{Context, Result, bail};
use gothic_formats::{
    vfs::VirtualFileSystem, xact::Actor, xcmsh::Xcmsh, ximg::Ximg, xlmsh::Xlmsh, xnvmsh::Xnvmsh,
};
use image::RgbaImage;
use std::path::Path;

pub struct Original {
    pub vfs: VirtualFileSystem,
}

pub struct StaticSub {
    pub material: String,
    pub positions: Vec<V3>,
    pub normals: Vec<V3>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

impl Original {
    pub fn open(data_dir: &Path) -> Result<Self> {
        Ok(Self {
            vfs: VirtualFileSystem::open_data_dir(data_dir)
                .with_context(|| format!("mounting {}", data_dir.display()))?,
        })
    }

    pub fn read(&mut self, name: &str) -> Result<Vec<u8>> {
        let slot = self
            .vfs
            .resolve_name(name)
            .with_context(|| format!("{name} not found in the installation"))?;
        self.vfs.read(slot)
    }

    /// LOD-0 submeshes of a static `.xcmsh`, following an `.xlmsh` to its level 0 mesh.
    /// Returns the resolved mesh name too.
    pub fn static_mesh(&mut self, name: &str) -> Result<(String, Vec<StaticSub>)> {
        let mut name = name.to_string();
        if name.to_ascii_lowercase().ends_with(".xlmsh") {
            let lod = Xlmsh::parse(&self.read(&name)?)?;
            let level = lod
                .levels
                .iter()
                .find(|l| l.source_index == 0)
                .context("xlmsh without level 0")?;
            name = level.mesh.clone();
        }
        let mesh = Xcmsh::parse(&self.read(&name)?)?;
        let subs = mesh
            .submeshes
            .into_iter()
            .filter(|s| !s.positions.is_empty() && !s.indices.is_empty())
            .map(|s| {
                let n = s.positions.len();
                StaticSub {
                    material: s.material,
                    normals: if s.normals.len() == n { s.normals } else { vec![[0.0, 1.0, 0.0]; n] },
                    uvs: if s.uvs.len() == n { s.uvs } else { vec![[0.0; 2]; n] },
                    positions: s.positions,
                    indices: s.indices,
                }
            })
            .collect::<Vec<_>>();
        if subs.is_empty() {
            bail!("{name} has no geometry");
        }
        Ok((name, subs))
    }

    /// The game's own collision mesh (`<stem>_COL.xnvmsh`), triangles in centimetres.
    pub fn collision(&mut self, mesh_name: &str) -> Option<Vec<[V3; 3]>> {
        let base = mesh_name.rsplit(['/', '\\']).next()?.to_ascii_lowercase();
        let stem = base.rsplit_once('.').map_or(base.as_str(), |(s, _)| s);
        let bytes = self.read(&format!("{stem}_col.xnvmsh")).ok()?;
        Xnvmsh::parse(&bytes).ok().map(|n| n.triangles_cm())
    }

    pub fn actor(&mut self, stem: &str) -> Result<Actor> {
        let stem = stem.trim_end_matches(".xact").trim_end_matches(".FXA");
        Actor::parse(&self.read(&format!("{stem}.xact"))?)
    }

    pub fn texture(&mut self, stem: &str) -> Result<RgbaImage> {
        let bytes = self.read(&format!("{stem}.ximg"))?;
        let img = Ximg::parse(&bytes)?;
        let mip = img.mips().first().context("texture without mips")?;
        crate::tex::decode(&img.fourcc, mip, img.width, img.height)
    }
}

pub fn soup(positions: &[V3], indices: &[u32]) -> Vec<[V3; 3]> {
    indices
        .chunks_exact(3)
        .filter(|t| t.iter().all(|&i| (i as usize) < positions.len()))
        .map(|t| [positions[t[0] as usize], positions[t[1] as usize], positions[t[2] as usize]])
        .collect()
}
