//! Cache layout, manifest and the runtime reader ([`Cache`]).
//!
//! ```text
//! <root>/<profile>/manifest.json     provenance + per-artifact keys
//!                  meshes/<id>.gmsh  textures/<id>.gtex  actors/<id>.gact  motions/<id>.gmot
//!                  cells/<cx>_<cz>.gcell  materials.gmat  collision.gcol  db.gdb
//! ```
use crate::actor::{ActorData, MotionData};
use crate::container::{Container, Kind};
use crate::ids::{AssetId, COOKER_VERSION};
use crate::material::{MaterialRec, MaterialTable};
use crate::mesh::MeshData;
use crate::mmap::Blob;
use crate::texture::TextureData;
use crate::world::{CellData, CollisionMesh, CollisionPack};
use anyhow::{Context, Result};
use gothic_gameplay::db::GameDatabase;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

pub fn mesh_path(id: AssetId) -> String {
    format!("meshes/{}.gmsh", id.hex())
}
pub fn texture_path(id: AssetId) -> String {
    format!("textures/{}.gtex", id.hex())
}
pub fn actor_path(id: AssetId) -> String {
    format!("actors/{}.gact", id.hex())
}
pub fn motion_path(id: AssetId) -> String {
    format!("motions/{}.gmot", id.hex())
}
pub fn cell_path(cx: i32, cz: i32) -> String {
    format!("cells/{cx}_{cz}.gcell")
}
pub const MATERIALS_PATH: &str = "materials.gmat";
pub const COLLISION_PATH: &str = "collision.gcol";
pub const DB_PATH: &str = "db.gdb";
pub const MANIFEST_PATH: &str = "manifest.json";

/// Default cache root: `<user data dir>/GothicReforged/cache` (never inside the repository).
pub fn default_cache_root() -> PathBuf {
    dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join("GothicReforged")
        .join("cache")
}

/// One cooked artifact in the manifest.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Entry {
    /// Source asset name (or description for aggregate artifacts).
    pub name: String,
    /// Winning source archive for provenance (`""` for aggregates).
    pub source: String,
    /// Content-addressing key: hash(source stamp, cooker version, profile settings).
    pub key: u64,
    /// XXH3 of the cooked bytes.
    pub content_hash: u64,
    pub bytes: u64,
    /// Assets this artifact needs (materials for meshes, textures for materials, ...).
    pub refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Manifest {
    pub cooker_version: u32,
    pub profile: String,
    pub profile_settings_hash: u64,
    pub source_root: String,
    /// Hash of every archive's name, size and mtime at cook time.
    pub source_fingerprint: u64,
    pub cooked_at_unix: u64,
    pub region: Option<String>,
    pub entries: BTreeMap<String, Entry>,
    /// Terrain cell bounds (Bevy convention) keyed by mesh name; avoids re-parsing.
    pub terrain_bounds: BTreeMap<String, TerrainBounds>,
    pub failures: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TerrainBounds {
    pub key: u64,
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Manifest {
    pub fn load(root: &Path) -> Option<Self> {
        let m: Manifest =
            serde_json::from_slice(&std::fs::read(root.join(MANIFEST_PATH)).ok()?).ok()?;
        (m.cooker_version == COOKER_VERSION).then_some(m)
    }
    pub fn save(&self, root: &Path) -> Result<()> {
        write_atomic(&root.join(MANIFEST_PATH), &serde_json::to_vec_pretty(self)?)
    }
    pub fn total_bytes(&self) -> u64 {
        self.entries.values().map(|e| e.bytes).sum()
    }
}

/// Write via a temp file + rename so readers never see a partial artifact.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!(
        "{}.tmp{}",
        path.extension().and_then(|e| e.to_str()).unwrap_or(""),
        std::process::id()
    ));
    std::fs::write(&tmp, bytes).with_context(|| format!("write {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("rename to {}", path.display()))?;
    Ok(())
}

/// Memory-mapped runtime access to a cooked profile directory.
pub struct Cache {
    root: PathBuf,
    materials: OnceLock<Option<MaterialTable>>,
    collision: OnceLock<Option<CollisionPack>>,
}

impl Cache {
    /// Open `<root>` (a profile directory containing `manifest.json`).
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        anyhow::ensure!(
            root.join(MANIFEST_PATH).is_file(),
            "no cooked cache at {} (run `inspector cook`)",
            root.display()
        );
        Ok(Self {
            root,
            materials: OnceLock::new(),
            collision: OnceLock::new(),
        })
    }

    /// Open the default per-user cache of `profile`.
    pub fn open_default(profile: &str) -> Result<Self> {
        Self::open(default_cache_root().join(profile))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn manifest(&self) -> Option<Manifest> {
        Manifest::load(&self.root)
    }

    fn blob(&self, rel: &str) -> Result<Arc<Blob>> {
        Ok(Arc::new(Blob::map(&self.root.join(rel))?))
    }

    pub fn load_mesh(&self, id: AssetId) -> Result<MeshData> {
        MeshData::parse(self.blob(&mesh_path(id))?)
    }
    pub fn load_mesh_named(&self, name: &str) -> Result<MeshData> {
        self.load_mesh(AssetId::of(name))
    }
    pub fn has_mesh(&self, id: AssetId) -> bool {
        self.root.join(mesh_path(id)).is_file()
    }
    pub fn load_texture(&self, id: AssetId) -> Result<TextureData> {
        TextureData::parse(self.blob(&texture_path(id))?)
    }
    pub fn load_texture_named(&self, name: &str) -> Result<TextureData> {
        self.load_texture(AssetId::of(name))
    }
    /// `Ok(None)` when the cell has no content (no file was cooked for it).
    pub fn load_cell(&self, cx: i32, cz: i32) -> Result<Option<CellData>> {
        let p = self.root.join(cell_path(cx, cz));
        if !p.is_file() {
            return Ok(None);
        }
        CellData::parse(Arc::new(Blob::map(&p)?)).map(Some)
    }
    pub fn load_actor(&self, id: AssetId) -> Result<ActorData> {
        ActorData::parse(self.blob(&actor_path(id))?)
    }
    pub fn load_motion(&self, id: AssetId) -> Result<MotionData> {
        MotionData::parse(self.blob(&motion_path(id))?)
    }
    /// Deserialize the cooked [`GameDatabase`] (no INI/Genome parsing).
    pub fn load_db(&self) -> Result<GameDatabase> {
        let blob = self.blob(DB_PATH)?;
        let c = Container::parse(blob, Kind::Database)?;
        crate::db::decode(c.raw(0))
    }

    fn materials_table(&self) -> Option<&MaterialTable> {
        self.materials
            .get_or_init(|| MaterialTable::parse(self.blob(MATERIALS_PATH).ok()?).ok())
            .as_ref()
    }
    pub fn material(&self, id: u64) -> Option<MaterialRec> {
        self.materials_table()?.get(id).copied()
    }
    pub fn material_count(&self) -> usize {
        self.materials_table().map_or(0, |t| t.records().len())
    }
    fn collision_pack(&self) -> Option<&CollisionPack> {
        self.collision
            .get_or_init(|| CollisionPack::parse(self.blob(COLLISION_PATH).ok()?).ok())
            .as_ref()
    }
    /// Collision mesh of a placed prop (`Instance::collision`).
    pub fn collision(&self, id: u64) -> Option<CollisionMesh<'_>> {
        self.collision_pack()?.get(id)
    }
}
