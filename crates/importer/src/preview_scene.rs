//! Explicit single-archive geometry preview; no inferred patch precedence.
use anyhow::{Context, Result, ensure};
use gothic_formats::{PakArchive, world_node::WorldDocument, xcmsh::Xcmsh, xlmsh::Xlmsh};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

const MAX_PAYLOAD_BYTES: u64 = 128 * 1024 * 1024;
const MAX_GEOMETRY_BYTES: usize = 128 * 1024 * 1024;
const MAX_INSTANCES: usize = 10_000;
const MAX_MESHES: usize = 4096;

#[derive(Debug, Serialize)]
pub struct PreviewScene {
    pub meshes: Vec<PreviewMesh>,
    pub instances: Vec<PreviewInstance>,
    /// Missing, ambiguous, corrupt, or unsupported resources skipped individually.
    pub issues: Vec<String>,
    /// Explicit preview policies; these are not resource failures.
    pub notes: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct PreviewMesh {
    /// Member path inside the single caller-selected archive.
    pub source_path: String,
    pub mesh: Xcmsh,
}

#[derive(Debug, Serialize)]
pub struct PreviewInstance {
    pub source_id: [u8; 20],
    pub name: String,
    pub world_matrix: [[f32; 4]; 4],
    pub mesh_index: usize,
}

/// Match references by case-insensitive basename in one explicit archive.
/// Meshes are decoded once, while each instance retains its authored transform.
/// Ordinary resource failures are issues; budget exhaustion fails the import.
pub fn import_preview(world: &WorldDocument, mesh_archive: &Path) -> Result<PreviewScene> {
    import_with_policy(world, mesh_archive, false)
}

/// Explicitly select XLMSH source level 0; original distance-based LOD behavior
/// remains unverified. Only one XLMSH indirection ending in XCMSH is supported.
pub fn import_preview_lod0(world: &WorldDocument, mesh_archive: &Path) -> Result<PreviewScene> {
    import_with_policy(world, mesh_archive, true)
}

fn import_with_policy(
    world: &WorldDocument,
    mesh_archive: &Path,
    lod0: bool,
) -> Result<PreviewScene> {
    let archive = PakArchive::open(mesh_archive).context("open explicit preview mesh archive")?;
    let basenames = index_basenames(archive.entries.iter().map(|entry| entry.path.as_str()));
    let mut loader = PreviewLoader {
        archive,
        basenames,
        cache: BTreeMap::new(),
        payload_bytes: 0,
        geometry_bytes: 0,
        lod0,
        scene: PreviewScene {
            meshes: Vec::new(),
            instances: Vec::new(),
            issues: Vec::new(),
            notes: if lod0 {
                vec!["Preview explicitly selects XLMSH source LOD0; original distance behavior is unverified. One explicit mesh archive is used, without inferred patch precedence.".into()]
            } else {
                vec![]
            },
        },
    };
    let mut instance_text_bytes = 0usize;
    for entity in &world.entities {
        let mut entity_meshes = BTreeSet::new();
        for reference in &entity.asset_references {
            if let Some(mesh_index) = loader.load(reference)? {
                if entity_meshes.insert(mesh_index) {
                    ensure!(
                        loader.scene.instances.len() < MAX_INSTANCES,
                        "preview instance budget exceeds 10000"
                    );
                    instance_text_bytes = instance_text_bytes
                        .checked_add(entity.name.len())
                        .context("preview name size overflow")?;
                    ensure!(
                        instance_text_bytes <= 16 * 1024 * 1024,
                        "preview instance name budget exceeds 16 MiB"
                    );
                    ensure!(
                        entity
                            .world_matrix
                            .iter()
                            .flatten()
                            .all(|value| value.is_finite()),
                        "preview instance has non-finite source matrix"
                    );
                    loader.scene.instances.push(PreviewInstance {
                        source_id: entity.source_id,
                        name: entity.name.clone(),
                        world_matrix: entity.world_matrix,
                        mesh_index,
                    });
                }
            }
        }
    }
    Ok(loader.scene)
}

struct PreviewLoader {
    archive: PakArchive,
    basenames: BTreeMap<String, Vec<usize>>,
    cache: BTreeMap<String, Option<usize>>,
    scene: PreviewScene,
    payload_bytes: u64,
    geometry_bytes: usize,
    lod0: bool,
}
impl PreviewLoader {
    fn load(&mut self, reference: &str) -> Result<Option<usize>> {
        ensure!(
            reference.len() <= 32768,
            "preview reference text exceeds ceiling"
        );
        let key = basename(reference).to_lowercase();
        if let Some(cached) = self.cache.get(&key) {
            return Ok(*cached);
        }
        ensure!(
            self.cache.len() < MAX_INSTANCES,
            "preview reference budget exceeded"
        );
        // Reserve before resolving an indirection; failed and in-progress keys cache None.
        self.cache.insert(key.clone(), None);
        let result = self.load_uncached(&key, reference)?;
        self.cache.insert(key, result);
        Ok(result)
    }
    fn load_uncached(&mut self, key: &str, reference: &str) -> Result<Option<usize>> {
        let is_lod = key.ends_with(".xlmsh") && self.lod0;
        if !key.ends_with(".xcmsh") && !is_lod {
            self.scene.issues.push(format!("unsupported preview resource: {reference}; explicit LOD0 option is required for XLMSH, and other formats remain unsupported"));
            return Ok(None);
        }
        let index = match resolve(&self.basenames, key) {
            Resolution::Missing => {
                self.scene
                    .issues
                    .push(format!("mesh not found in explicit archive: {reference}"));
                return Ok(None);
            }
            Resolution::Ambiguous => {
                self.scene.issues.push(format!(
                    "ambiguous mesh basename in explicit archive: {reference}"
                ));
                return Ok(None);
            }
            Resolution::Unique(index) => index,
        };
        let entry = &self.archive.entries[index];
        if entry.deleted() || entry.payloadless() {
            self.scene
                .issues
                .push(format!("mesh has no available payload: {}", entry.path));
            return Ok(None);
        }
        self.payload_bytes = self
            .payload_bytes
            .checked_add(entry.size)
            .context("preview payload size overflow")?;
        ensure!(
            self.payload_bytes <= MAX_PAYLOAD_BYTES,
            "preview decoded payload budget exceeds 128 MiB"
        );
        ensure!(
            entry.stored_size <= MAX_PAYLOAD_BYTES,
            "preview stored payload budget exceeds 128 MiB"
        );
        let source_path = entry.path.clone();
        let payload = match self.archive.read_payload(index) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.scene.issues.push(format!(
                    "unsupported or invalid mesh {source_path}: {error:#}"
                ));
                return Ok(None);
            }
        };
        if is_lod {
            let lod = match Xlmsh::parse(&payload) {
                Ok(lod) => lod,
                Err(error) => {
                    self.scene.issues.push(format!(
                        "unsupported or invalid LOD resource {source_path}: {error:#}"
                    ));
                    return Ok(None);
                }
            };
            let mut level0 = lod.levels.iter().filter(|level| level.source_index == 0);
            let Some(selected) = level0.next() else {
                self.scene
                    .issues
                    .push(format!("LOD resource has no source level 0: {source_path}"));
                return Ok(None);
            };
            if level0.next().is_some() {
                self.scene.issues.push(format!(
                    "LOD resource has ambiguous source level 0: {source_path}"
                ));
                return Ok(None);
            }
            let chosen = selected.mesh.clone();
            if !basename(&chosen).to_ascii_lowercase().ends_with(".xcmsh") {
                self.scene.issues.push(format!("nested, cyclic, or unsupported LOD0 reference in {source_path}: {chosen}; one indirection to XCMSH is required"));
                return Ok(None);
            }
            drop(lod);
            drop(payload);
            return self.load(&chosen);
        }
        match Xcmsh::parse(&payload) {
            Ok(mesh) => {
                ensure!(
                    self.scene.meshes.len() < MAX_MESHES,
                    "preview unique mesh budget exceeded"
                );
                self.geometry_bytes = self
                    .geometry_bytes
                    .checked_add(mesh_bytes(&mesh)?)
                    .context("preview geometry size overflow")?;
                ensure!(
                    self.geometry_bytes <= MAX_GEOMETRY_BYTES,
                    "preview retained geometry budget exceeds 128 MiB"
                );
                let mesh_index = self.scene.meshes.len();
                self.scene.meshes.push(PreviewMesh { source_path, mesh });
                Ok(Some(mesh_index))
            }
            Err(error) => {
                self.scene.issues.push(format!(
                    "unsupported or invalid mesh {source_path}: {error:#}"
                ));
                Ok(None)
            }
        }
    }
}

fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or_default()
}

fn index_basenames<'a>(paths: impl Iterator<Item = &'a str>) -> BTreeMap<String, Vec<usize>> {
    let mut names: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, path) in paths.enumerate() {
        if [".xcmsh", ".xlmsh"]
            .iter()
            .any(|extension| path.to_ascii_lowercase().ends_with(extension))
        {
            names
                .entry(basename(path).to_lowercase())
                .or_default()
                .push(index);
        }
    }
    names
}

#[derive(Debug, PartialEq, Eq)]
enum Resolution {
    Missing,
    Ambiguous,
    Unique(usize),
}
fn resolve(index: &BTreeMap<String, Vec<usize>>, key: &str) -> Resolution {
    match index.get(key).map(Vec::as_slice) {
        Some([index]) => Resolution::Unique(*index),
        Some(_) => Resolution::Ambiguous,
        None => Resolution::Missing,
    }
}

fn mesh_bytes(mesh: &Xcmsh) -> Result<usize> {
    let mut bytes = 0usize;
    for submesh in &mesh.submeshes {
        for (count, stride) in [
            (submesh.positions.capacity(), 12usize),
            (submesh.normals.capacity(), 12),
            (submesh.uvs.capacity(), 8),
            (submesh.indices.capacity(), 4),
            (submesh.material.capacity(), 1),
        ] {
            bytes = bytes
                .checked_add(
                    count
                        .checked_mul(stride)
                        .context("preview stream size overflow")?,
                )
                .context("preview mesh size overflow")?;
        }
        bytes = bytes
            .checked_add(std::mem::size_of_val(submesh))
            .context("preview mesh overhead overflow")?;
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gothic_formats::world_node::{WorldEntity, WorldKind};
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    #[test]
    fn basename_index_keeps_ambiguity_and_does_not_choose_first_match() {
        let paths = [
            "a/Bench.XCMSH",
            "b/bench.xcmsh",
            "Tree.xlmsh",
            "other/TORCH.xcmsh",
        ];
        let index = index_basenames(paths.into_iter());
        assert_eq!(resolve(&index, "bench.xcmsh"), Resolution::Ambiguous);
        assert_eq!(resolve(&index, "torch.xcmsh"), Resolution::Unique(3));
        assert_eq!(resolve(&index, "missing.xcmsh"), Resolution::Missing);
        assert_eq!(
            basename(r"C:\\source\Bench.XCMSH").to_lowercase(),
            "bench.xcmsh"
        );
        assert_eq!(basename("folder/bench.xcmsh"), "bench.xcmsh");
    }

    static COUNT: AtomicU64 = AtomicU64::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "gothic-preview-test-{}-{}",
                std::process::id(),
                COUNT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
    fn archive(size: u64) -> Vec<u8> {
        let mut bytes = Vec::new();
        for word in [0u32, 0x30563347, 0, 0, 1, 0] {
            bytes.extend(word.to_le_bytes());
        }
        for offset in [53u64; 3] {
            bytes.extend(offset.to_le_bytes());
        }
        bytes.extend(b"wrong");
        bytes.extend([0; 32]);
        for word in [0x10u32, 0, 0, 1] {
            bytes.extend(word.to_le_bytes());
        }
        bytes.extend([0; 32]);
        bytes.extend(0x20000u32.to_le_bytes());
        for value in [48u64, 5, size] {
            bytes.extend(value.to_le_bytes());
        }
        for word in [0u32, 0, 11] {
            bytes.extend(word.to_le_bytes());
        }
        bytes.extend(b"bench.xcmsh\0");
        bytes.extend(0u32.to_le_bytes());
        bytes
    }
    fn world() -> WorldDocument {
        let entities = (0..2)
            .map(|id| WorldEntity {
                source_id: [id; 20],
                source_offset: 0,
                name: "test".into(),
                world_matrix: [[0.0; 4]; 4],
                local_matrix: [[0.0; 4]; 4],
                asset_references: vec!["bench.xcmsh".into(), "unavailable.xlmsh".into()],
                property_sets: vec![],
            })
            .collect();
        WorldDocument {
            kind: WorldKind::Node,
            entities,
            edges: vec![],
        }
    }
    #[test]
    fn corrupt_mesh_failure_is_cached_and_unsupported_lod_is_reported() -> Result<()> {
        let fixture = Fixture::new();
        let path = fixture.0.join("mesh.pak");
        fs::write(&path, archive(5))?;
        let scene = import_preview(&world(), &path)?;
        assert!(scene.meshes.is_empty() && scene.instances.is_empty());
        assert_eq!(scene.issues.len(), 2);
        assert!(scene.issues[0].contains("invalid mesh"));
        assert!(scene.issues[1].contains("unavailable.xlmsh"));
        Ok(())
    }
    #[test]
    fn decoded_budget_exhaustion_is_a_hard_error_before_reading_payload() -> Result<()> {
        let fixture = Fixture::new();
        let path = fixture.0.join("large.pak");
        fs::write(&path, archive(MAX_PAYLOAD_BYTES + 1))?;
        let error = import_preview(&world(), &path).unwrap_err();
        assert!(error.to_string().contains("payload budget"));
        Ok(())
    }
}
