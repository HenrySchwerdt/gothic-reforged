//! Cached loading of actors, motions, textures and static meshes.
//!
//! [`ActorLibrary`] owns the archive handles and every cache; loading methods take
//! [`AssetStores`] (mutable access to the Bevy asset collections) so the same library works
//! from a startup system, a streaming system or an exclusive world.
use crate::{
    animator::Motion,
    clip::{Clip, ClipBinding},
    locomotion::{LocoKey, resolve_motion},
    names::{MotionCatalog, MotionName},
    rig::{NONE, Rig},
};
use anyhow::{Context, Result, anyhow};
use bevy::{
    asset::RenderAssetUsages,
    camera::primitives::Aabb,
    ecs::resource::Resource,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    math::{Affine3A, Quat, Vec3},
    mesh::{
        Indices, Mesh, PrimitiveTopology, VertexAttributeValues,
        skinning::SkinnedMeshInverseBindposes,
    },
    prelude::{AlphaMode, Assets, Color, Handle, Image, StandardMaterial},
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use gothic_formats::{
    PakArchive,
    xact::{Actor, ActorSubmesh},
    xcmsh::Xcmsh,
    ximg::Ximg,
    xmot::Motion as RawMotion,
    xshmat::Xshmat,
};
use std::{
    collections::{BTreeSet, HashMap},
    path::Path,
    sync::Arc,
};

/// Diffuse texture stem and `(blend mode, mask reference)` of a shader material.
type MaterialInfo = (Option<String>, Option<(u32, u8)>);

/// Mutable access to the Bevy asset collections the loaders fill.
pub struct AssetStores<'a> {
    pub meshes: &'a mut Assets<Mesh>,
    pub materials: &'a mut Assets<StandardMaterial>,
    pub images: &'a mut Assets<Image>,
    pub bindposes: &'a mut Assets<SkinnedMeshInverseBindposes>,
}

/// One skinned mesh group of an actor file, ready to be instanced by any number of NPCs.
pub struct PartAsset {
    pub name: String,
    /// The part's own skeleton (names, hierarchy, bind pose).
    pub rig: Arc<Rig>,
    /// Part node index of every compact joint used by the meshes.
    pub joint_nodes: Vec<u16>,
    pub inverse_bindposes: Handle<SkinnedMeshInverseBindposes>,
    pub meshes: Vec<PartMesh>,
    pub aabb: Aabb,
}

pub struct PartMesh {
    pub mesh: Handle<Mesh>,
    pub material: Handle<StandardMaterial>,
}

/// A static (unskinned) mesh such as a weapon.
pub struct StaticAsset {
    pub name: String,
    pub meshes: Vec<PartMesh>,
    pub bounds_min: Vec3,
    pub bounds_max: Vec3,
}

pub struct Composition {
    pub key: String,
    /// Master skeleton (the body's).
    pub rig: Arc<Rig>,
    pub parts: Vec<Arc<PartAsset>>,
    /// Rig node of every joint entity.
    pub joint_nodes: Arc<[u16]>,
    /// For each part: compact joint -> index into `joint_nodes`.
    pub part_joints: Vec<Vec<u16>>,
    pub plan: Arc<[crate::rig::EvalStep]>,
    /// Rotation about Y that turns the authored animation facing onto the entity's forward
    /// axis ([`crate::locomotion::FORWARD`]).
    pub facing_fix: Quat,
    /// Lower-case node name to joint index, for slots and any other joint.
    pub joint_by_name: HashMap<String, u16>,
    /// `Slot_*` nodes that exist only in an extra part (`Slot_Hair`, `Slot_Beard` live in the
    /// head actor): lower-case name to (joint index, offset from that joint, bind pose).
    pub slot_mounts: HashMap<String, (u16, Affine3A)>,
}

#[derive(Resource)]
pub struct ActorLibrary {
    animation: PakArchive,
    material_pak: PakArchive,
    image_pak: PakArchive,
    mesh_pak: PakArchive,
    xact_index: HashMap<String, usize>,
    xmot_index: HashMap<String, usize>,
    material_index: HashMap<String, usize>,
    image_index: HashMap<String, usize>,
    mesh_index: HashMap<String, usize>,
    pub catalog: MotionCatalog,
    parts: HashMap<String, Arc<PartAsset>>,
    clips: HashMap<String, Arc<Clip>>,
    bindings: HashMap<(String, String), Arc<ClipBinding>>,
    textures: HashMap<String, Option<Handle<Image>>>,
    shared_materials: HashMap<(String, u32, u8), Handle<StandardMaterial>>,
    statics: HashMap<String, Arc<StaticAsset>>,
    compositions: HashMap<String, Arc<Composition>>,
    loco: HashMap<(String, LocoKey), Option<String>>,
    family_alias: HashMap<String, String>,
    data_dir: std::path::PathBuf,
    prefetch: Option<Prefetch>,
}

/// Decoded data a worker thread prepared ahead of a spawn: parsed actors (by archive index) and
/// decoded texture images (by `<stem>.ximg` key). The main thread takes what is there and loads
/// the rest inline, so a missed prefetch costs time but never correctness.
#[derive(Default)]
struct Shared {
    actors: std::sync::Mutex<HashMap<usize, Arc<Actor>>>,
    images: std::sync::Mutex<HashMap<String, Option<Image>>>,
}

struct Prefetch {
    tx: std::sync::mpsc::Sender<Vec<String>>,
    shared: Arc<Shared>,
    /// Parts queued and not finished yet.
    pending: Arc<std::sync::atomic::AtomicUsize>,
}

/// Main-thread time spent in the cold paths of the library (nanoseconds, call count): actor
/// read + parse, texture decode, material file, clip decode, mesh build. For the frame-time
/// reports of the open world (`profile()`).
static PROF: [(std::sync::atomic::AtomicU64, std::sync::atomic::AtomicU64); 5] = [const {
    (
        std::sync::atomic::AtomicU64::new(0),
        std::sync::atomic::AtomicU64::new(0),
    )
}; 5];
const PROF_NAMES: [&str; 5] = ["actor parse", "texture", "material file", "clip", "mesh build"];

struct Prof(usize, std::time::Instant);
impl Prof {
    fn start(i: usize) -> Self {
        Self(i, std::time::Instant::now())
    }
}
impl Drop for Prof {
    fn drop(&mut self) {
        use std::sync::atomic::Ordering::Relaxed;
        PROF[self.0].0.fetch_add(self.1.elapsed().as_nanos() as u64, Relaxed);
        PROF[self.0].1.fetch_add(1, Relaxed);
    }
}

/// `(what, total ms, calls)` of the cold library paths since start.
pub fn profile() -> Vec<(&'static str, f64, u64)> {
    use std::sync::atomic::Ordering::Relaxed;
    PROF.iter()
        .zip(PROF_NAMES)
        .map(|((ns, n), name)| (name, ns.load(Relaxed) as f64 / 1.0e6, n.load(Relaxed)))
        .collect()
}

fn basename(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_lowercase()
}

/// Stand-in image key for a missing actor texture (all keys lower case, with `.ximg`).
fn alternative_texture(key: &str, index: &HashMap<String, usize>) -> Option<String> {
    let stem = key.strip_suffix(".ximg")?;
    // `..._01` / `..._02` -> `..._s1` / `..._s2`.
    if let Some((head, tail)) = stem.rsplit_once('_')
        && let Some(n) = tail.strip_prefix('0').filter(|n| n.len() == 1)
    {
        let c = format!("{head}_s{n}.ximg");
        if index.contains_key(&c) {
            return Some(c);
        }
    }
    let by_keyword: &[(&str, &str)] = &[
        ("helmet", "g3_armor_helm_myrtana_01_diffuse_s1.ximg"),
        ("helm", "g3_armor_helm_myrtana_01_diffuse_s1.ximg"),
        ("cloth", "g3_human_slave_cloth_diffuse_s1.ximg"),
        ("gargoly", "g3_human_slave_body_diffuse_s1.ximg"),
        ("slave_body", "g3_human_slave_body_diffuse_s1.ximg"),
        ("smith_body", "g3_human_smith_body_diffuse_s1.ximg"),
    ];
    by_keyword
        .iter()
        .find(|(k, v)| stem.contains(k) && index.contains_key(*v))
        .map(|(_, v)| (*v).to_string())
}

fn first_index(pak: &PakArchive, suffix: Option<&str>) -> HashMap<String, usize> {
    pak.entries
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, e)| !e.deleted() && !e.payloadless())
        .filter(|(_, e)| suffix.is_none_or(|s| e.path.to_lowercase().ends_with(s)))
        .map(|(i, e)| (basename(&e.path), i))
        .collect()
}

impl ActorLibrary {
    /// Opens the four archives under `data_dir` (`_compiledAnimation/Material/Image/Mesh.pak`).
    pub fn open(data_dir: &Path) -> Result<Self> {
        let animation = PakArchive::open(data_dir.join("_compiledAnimation.pak"))
            .context("_compiledAnimation.pak")?;
        let material_pak = PakArchive::open(data_dir.join("_compiledMaterial.pak"))
            .context("_compiledMaterial.pak")?;
        let image_pak =
            PakArchive::open(data_dir.join("_compiledImage.pak")).context("_compiledImage.pak")?;
        let mesh_pak =
            PakArchive::open(data_dir.join("_compiledMesh.pak")).context("_compiledMesh.pak")?;
        let xact_index = first_index(&animation, Some(".xact"));
        let xmot_index: HashMap<String, usize> = first_index(&animation, Some(".xmot"));
        let stems: Vec<String> = animation
            .entries
            .iter()
            .filter(|e| e.path.to_lowercase().ends_with(".xmot") && !e.deleted())
            .map(|e| e.path.trim_end_matches(".xmot").to_string())
            .collect();
        let catalog = MotionCatalog::new(stems.iter().map(String::as_str));
        let material_index = first_index(&material_pak, None);
        let image_index = first_index(&image_pak, None);
        let mesh_index = first_index(&mesh_pak, Some(".xcmsh"));
        Ok(Self {
            animation,
            material_pak,
            image_pak,
            mesh_pak,
            xact_index,
            xmot_index,
            material_index,
            image_index,
            mesh_index,
            catalog,
            parts: HashMap::new(),
            clips: HashMap::new(),
            bindings: HashMap::new(),
            textures: HashMap::new(),
            shared_materials: HashMap::new(),
            statics: HashMap::new(),
            compositions: HashMap::new(),
            loco: HashMap::new(),
            family_alias: HashMap::new(),
            data_dir: data_dir.to_path_buf(),
            prefetch: None,
        })
    }

    /// Starts the background decoder: it opens its own archive handles and parses the actors
    /// and decodes the textures of the parts passed to [`Self::prefetch_parts`], so the spawn
    /// on the main thread finds them ready (actor read + parse and texture decode were about
    /// 90 % of a cold character build).
    pub fn start_prefetch(&mut self) {
        if self.prefetch.is_some() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel::<Vec<String>>();
        let shared = Arc::new(Shared::default());
        let pending = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (dir, out, left) = (self.data_dir.clone(), shared.clone(), pending.clone());
        let started = std::thread::Builder::new()
            .name("actor-prefetch".into())
            .spawn(move || {
                let Ok(mut lib) = ActorLibrary::open(&dir) else {
                    return;
                };
                let mut seen: std::collections::HashSet<String> = Default::default();
                while let Ok(names) = rx.recv() {
                    for name in names {
                        if seen.insert(name.to_lowercase()) {
                            lib.prefetch_one(&name, &out);
                        }
                        left.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                    }
                }
            });
        if started.is_ok() {
            self.prefetch = Some(Prefetch {
                tx,
                shared,
                pending,
            });
        }
    }

    /// Queue parts (`name` or `name@M<n>`) for the background decoder.
    pub fn prefetch_parts(&self, names: &[String]) {
        if let Some(p) = &self.prefetch {
            p.pending
                .fetch_add(names.len(), std::sync::atomic::Ordering::Relaxed);
            let _ = p.tx.send(names.to_vec());
        }
    }

    /// Parts the background decoder has not finished yet.
    pub fn prefetch_pending(&self) -> usize {
        self.prefetch
            .as_ref()
            .map_or(0, |p| p.pending.load(std::sync::atomic::Ordering::Relaxed))
    }

    /// Worker side: parse the actor, decode the textures its materials name.
    fn prefetch_one(&mut self, name: &str, out: &Shared) {
        let (base, skin) = match name.rsplit_once("@M") {
            Some((n, s)) if s.chars().all(|c| c.is_ascii_digit()) && !s.is_empty() => {
                (n, s.parse::<i32>().ok())
            }
            _ => (name, None),
        };
        let Ok(at) = self.find_actor(base) else {
            return;
        };
        let Ok(actor) = self.parse_actor(at) else {
            return;
        };
        let actor = Arc::new(actor);
        let mut used: BTreeSet<usize> = BTreeSet::new();
        for mesh in &actor.meshes {
            for sub in &mesh.submeshes {
                if sub.material != 0 && (sub.material as usize) < actor.materials.len() {
                    used.insert(sub.material as usize);
                }
            }
        }
        for index in used {
            let (stem, _, _) = self.material_stem(&actor, index, skin);
            let Some(stem) = stem else { continue };
            let key = format!("{}.ximg", stem.to_lowercase());
            if self.textures.contains_key(&key) {
                continue;
            }
            // Marks the key as handled on this side (the handle itself stays `None`).
            self.textures.insert(key.clone(), None);
            let image = self.load_texture_image(&key, &stem);
            if let Ok(mut m) = out.images.lock() {
                m.insert(key, image);
            }
        }
        if let Ok(mut m) = out.actors.lock() {
            m.entry(at).or_insert(actor);
        }
    }

    /// Read and parse the actor at archive index `at` (remaster override applied).
    fn parse_actor(&mut self, at: usize) -> Result<Actor> {
        let _p = Prof::start(0);
        let path = self.animation.entries[at].path.clone();
        let mut actor = Actor::parse(&self.animation.read_payload(at)?)
            .map_err(|e| e.context(format!("parsing {path}")))?;
        if let Some(ov) = gothic_formats::remaster::Profile::global()
            .and_then(|p| p.mesh(gothic_formats::remaster::OverrideKind::Actor, &path))
        {
            apply_remaster(&mut actor, &ov);
        }
        Ok(actor)
    }

    /// Motion family (the actor token of the motion names) of the skeleton whose animated
    /// nodes carry the prefix `node_family`. Usually the two agree; where they differ in the
    /// data (`Aligator` nodes, `Alligator` motions; `Keiler` nodes, `Boar` motions) the family
    /// is found from the node names the family's own clips animate.
    pub fn motion_family(&mut self, node_family: &str) -> String {
        let lower = node_family.to_lowercase();
        if self.catalog.motions(&lower).next().is_some() {
            return node_family.to_string();
        }
        if self.family_alias.is_empty() {
            let families: Vec<String> = self
                .catalog
                .families()
                .into_iter()
                .map(str::to_string)
                .collect();
            for family in families {
                let Some(name) = self
                    .catalog
                    .motions(&family)
                    .next()
                    .map(|(n, _)| n.to_string())
                else {
                    continue;
                };
                let Ok(clip) = self.clip(&name) else { continue };
                let prefix = clip
                    .nodes
                    .iter()
                    .filter(|n| !crate::rig::is_helper(&n.name) && !n.name.starts_with("Slot_"))
                    .find_map(|n| n.name.split('_').next().map(str::to_lowercase));
                if let Some(prefix) = prefix
                    && prefix != family
                {
                    self.family_alias.insert(prefix, family);
                }
            }
            self.family_alias.insert(String::new(), String::new());
        }
        self.family_alias
            .get(&lower)
            .cloned()
            .unwrap_or_else(|| node_family.to_string())
    }

    /// Names (without extension) of all installed actors passing `filter`, sorted.
    pub fn actor_names(&self, filter: impl Fn(&str) -> bool) -> Vec<String> {
        let mut names: Vec<String> = self
            .xact_index
            .values()
            .map(|&i| {
                self.animation.entries[i]
                    .path
                    .trim_end_matches(".xact")
                    .to_string()
            })
            .filter(|n| filter(n))
            .collect();
        names.sort();
        names
    }

    pub fn has_actor(&self, name: &str) -> bool {
        self.xact_index
            .contains_key(&format!("{}.xact", name.to_lowercase()))
    }

    pub fn has_motion(&self, name: &str) -> bool {
        self.xmot_index
            .contains_key(&format!("{}.xmot", name.to_lowercase()))
            || self.clips.contains_key(&name.to_lowercase())
    }

    /// Registers a clip that does not come from an `.xmot` (for example a locally generated
    /// clip pack). It is found by `clip` / `motion` / `has_motion` under its lower-cased name
    /// and replaces a clip of the same name.
    pub fn register_clip(&mut self, clip: Clip) {
        let key = clip.name.to_lowercase();
        self.bindings.retain(|(_, c), _| *c != key);
        self.clips.insert(key, Arc::new(clip));
    }

    /// Loads a clip pack file and registers every clip in it; returns the pack so the caller
    /// can read its timing metadata.
    pub fn load_clip_pack(&mut self, path: &Path) -> Result<crate::clip_pack::ClipPack> {
        let pack = crate::clip_pack::ClipPack::load(path)
            .with_context(|| format!("clip pack {}", path.display()))?;
        for c in &pack.clips {
            // Packs are generated against the authored (left-handed) skeleton.
            let mut clip = c.clip.clone();
            clip.mirror_x();
            self.register_clip(clip);
        }
        Ok(pack)
    }

    fn find_actor(&self, wanted: &str) -> Result<usize> {
        let lower = wanted.to_lowercase();
        let exact = format!("{}.xact", lower.trim_end_matches(".xact"));
        if let Some(&i) = self.xact_index.get(&exact) {
            return Ok(i);
        }
        self.xact_index
            .iter()
            .filter(|(k, _)| k.contains(&lower))
            .min_by_key(|(k, _)| k.len())
            .map(|(_, &i)| i)
            .ok_or_else(|| anyhow!("no .xact actor matches {wanted:?}"))
    }

    // ---- textures and materials ------------------------------------------------------

    fn texture(&mut self, stem: &str, images: &mut Assets<Image>) -> Option<Handle<Image>> {
        let key = format!("{}.ximg", stem.to_lowercase());
        if let Some(cached) = self.textures.get(&key) {
            return cached.clone();
        }
        let ready = self
            .prefetch
            .as_ref()
            .and_then(|p| p.shared.images.lock().ok()?.remove(&key));
        let image = match ready {
            Some(image) => image,
            None => self.load_texture_image(&key, stem),
        };
        let handle = image.map(|i| images.add(i));
        self.textures.insert(key, handle.clone());
        handle
    }

    /// Decode a texture to an image (no asset collection needed; also runs on the worker).
    fn load_texture_image(&mut self, key: &str, stem: &str) -> Option<Image> {
        // Local remaster profile first (see `gothic_formats::remaster`).
        if let Some(t) = gothic_formats::remaster::Profile::global().and_then(|p| p.texture(stem)) {
            let data: Vec<u8> = t.mips.concat();
            return bc_image(t.width, t.height, &t.fourcc, data, t.mips.len() as u32);
        }
        let _p = Prof::start(1);
        let at = self
            .image_index
            .get(key)
            .or_else(|| {
                // Variants: `_01` -> `_S1`, then keyword stand-ins (helmets, cloth).
                let alt = alternative_texture(key, &self.image_index)?;
                eprintln!("animation: texture {key} -> {alt}");
                self.image_index.get(&alt)
            })
            .copied();
        let Some(at) = at else {
            eprintln!("animation: texture {key} not in the image archive");
            return None;
        };
        let bytes = self.image_pak.read_payload(at).ok()?;
        let ximg = Ximg::parse(&bytes).ok()?;
        let image = texture_image(&ximg);
        if image.is_none() {
            eprintln!("animation: texture {key} has an unsupported format");
        }
        image
    }

    /// Diffuse stem and blend hint of a shader material file.
    fn material_file(&mut self, file: &str) -> Option<MaterialInfo> {
        let _p = Prof::start(2);
        let at = *self.material_index.get(&file.to_lowercase())?;
        let bytes = self.material_pak.read_payload(at).ok()?;
        let parsed = Xshmat::parse(&bytes).ok()?;
        Some((
            parsed.diffuse().map(|d| d.stem().to_string()),
            parsed
                .blend_mode
                .map(|m| (m, parsed.mask_reference.unwrap_or(128))),
        ))
    }

    /// Diffuse stem (switch applied) and blend hint of an actor material, plus whether the
    /// switch found its variant image.
    fn material_stem(
        &mut self,
        actor: &Actor,
        index: usize,
        switch: Option<i32>,
    ) -> (Option<String>, Option<(u32, u8)>, bool) {
        let material = &actor.materials[index];
        // The string table order of material files does not always follow the material
        // order (heads), so a file named like the material wins over the positional entry.
        let by_name = format!("{}.xshmat", material.name.trim_end_matches('|'));
        let listed = if actor
            .material_files
            .iter()
            .any(|f| f.eq_ignore_ascii_case(&by_name))
        {
            by_name
        } else {
            actor
                .material_file(index)
                .map(str::to_string)
                .unwrap_or(by_name)
        };
        // The listed file can be a generic exporter name that belongs to another model, so the
        // layer texture's own `_A.xshmat` / `.xshmat` are tried first.
        let mut candidates = Vec::new();
        if let Some(texture) = material.diffuse_texture() {
            candidates.push(format!("{texture}_A.xshmat"));
            candidates.push(format!("{texture}.xshmat"));
        }
        candidates.push(listed);
        let found = candidates
            .iter()
            .find(|c| self.material_index.contains_key(&c.to_lowercase()))
            .cloned();
        let (mut stem, blend) = found
            .and_then(|f| self.material_file(&f))
            .unwrap_or((None, None));
        if stem.is_none() {
            stem = material.diffuse_texture().map(str::to_string);
        }
        let mut variant_found = true;
        if let (Some(base), Some(sw)) = (stem.as_deref(), switch) {
            let switched = self.switch_stem(base, sw);
            variant_found = switched != base || base.rsplit_once("_S").is_none() || sw == 0;
            stem = Some(switched);
        }
        (stem, blend, variant_found)
    }

    fn actor_material(
        &mut self,
        actor: &Actor,
        index: usize,
        path: &str,
        switch: Option<i32>,
        stores: &mut AssetStores,
    ) -> Handle<StandardMaterial> {
        let (stem, blend, _) = self.material_stem(actor, index, switch);
        let material = &actor.materials[index];
        let resolves = stem.as_ref().is_some_and(|s| {
            self.image_index
                .contains_key(&format!("{}.ximg", s.to_lowercase()))
        });
        if stem.is_none() || !resolves {
            eprintln!(
                "animation: material {:?} of {path}: no resolvable texture (stem {:?})",
                material.name, stem
            );
        }
        self.shared_material(stem, blend, stores)
    }

    /// Texture problems of actor `name` (optionally `@M<n>`): materials without a resolvable
    /// image and switches whose variant image does not exist. Empty when everything resolves.
    pub fn texture_problems(&mut self, name: &str) -> Vec<String> {
        let (base, switch) = match name.rsplit_once("@M") {
            Some((n, s)) if !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()) => {
                (n, s.parse::<i32>().ok())
            }
            _ => (name, None),
        };
        let mut out = Vec::new();
        let parsed = self
            .find_actor(base)
            .and_then(|at| Actor::parse(&self.animation.read_payload(at)?));
        let actor = match parsed {
            Ok(a) => a,
            Err(e) => return vec![format!("{base}: {e:#}")],
        };
        for index in 1..actor.materials.len() {
            let (stem, _, variant) = self.material_stem(&actor, index, switch);
            let ok = stem.as_ref().is_some_and(|s| {
                self.image_index
                    .contains_key(&format!("{}.ximg", s.to_lowercase()))
            });
            if !ok {
                out.push(format!("{base}: material {index} no image ({stem:?})"));
            } else if !variant {
                out.push(format!("variant-missing {base} material {index}"));
            }
        }
        out
    }

    /// Applies a `MaterialSwitch` to a texture stem: the trailing `_S<k>` variant index becomes
    /// `_S<switch + 1>` (switch 0 is the first variant). A stem without variant suffix, or a
    /// variant the archive does not hold, keeps the authored texture.
    pub fn switch_stem(&self, stem: &str, switch: i32) -> String {
        let Some((head, tail)) = stem.rsplit_once("_S") else {
            return stem.to_string();
        };
        if tail.is_empty() || !tail.chars().all(|c| c.is_ascii_digit()) || switch < 0 {
            return stem.to_string();
        }
        let wanted = format!("{head}_S{}", switch + 1);
        if self
            .image_index
            .contains_key(&format!("{}.ximg", wanted.to_lowercase()))
        {
            wanted
        } else {
            stem.to_string()
        }
    }

    /// True when the mesh archive holds `name` (`.xcmsh`, case-insensitive).
    pub fn has_mesh(&self, name: &str) -> bool {
        let key = name.to_lowercase();
        self.mesh_index
            .contains_key(&format!("{}.xcmsh", key.trim_end_matches(".xcmsh")))
    }

    fn shared_material(
        &mut self,
        stem: Option<String>,
        blend: Option<(u32, u8)>,
        stores: &mut AssetStores,
    ) -> Handle<StandardMaterial> {
        let key = (
            stem.clone().unwrap_or_default().to_lowercase(),
            blend.map_or(u32::MAX, |b| b.0),
            blend.map_or(0, |b| b.1),
        );
        if let Some(h) = self.shared_materials.get(&key) {
            return h.clone();
        }
        let texture = stem.and_then(|s| self.texture(&s, stores.images));
        let handle = stores.materials.add(StandardMaterial {
            base_color: if texture.is_some() {
                Color::WHITE
            } else {
                Color::srgb(0.6, 0.55, 0.5)
            },
            base_color_texture: texture,
            perceptual_roughness: 0.85,
            reflectance: 0.1,
            alpha_mode: alpha_mode(blend),
            cull_mode: None,
            ..Default::default()
        });
        self.shared_materials.insert(key, handle.clone());
        handle
    }

    // ---- actors -----------------------------------------------------------------------

    /// Loads (or returns the cached) skinned part `name` (a `.xact` actor).
    ///
    /// `name` may end in `@M<n>`: the actor's `MaterialSwitch` (see [`Self::switch_stem`]).
    pub fn part(&mut self, name: &str, stores: &mut AssetStores) -> Result<Arc<PartAsset>> {
        let (name, skin) = match name.rsplit_once("@M") {
            Some((n, s)) if s.chars().all(|c| c.is_ascii_digit()) && !s.is_empty() => {
                (n, s.parse::<i32>().ok())
            }
            _ => (name, None),
        };
        let at = self.find_actor(name)?;
        let path = self.animation.entries[at].path.clone();
        let key = format!("{}#{}", path.to_lowercase(), skin.unwrap_or(-1));
        if let Some(p) = self.parts.get(&key) {
            return Ok(p.clone());
        }
        let prefetched = self
            .prefetch
            .as_ref()
            .and_then(|p| p.shared.actors.lock().ok()?.remove(&at));
        let actor: Arc<Actor> = match prefetched {
            Some(a) => a,
            None => Arc::new(self.parse_actor(at)?),
        };
        let mut rig = Rig::from_actor(path.trim_end_matches(".xact"), &actor);
        rig.family = self.motion_family(&rig.family);
        let rig = Arc::new(rig);
        // Compact joint table: only nodes that carry weight.
        let mut used = BTreeSet::new();
        for mesh in &actor.meshes {
            for sub in &mesh.submeshes {
                for (j, w) in sub.joints.iter().zip(&sub.weights) {
                    for k in 0..4 {
                        if w[k] != 0.0 {
                            used.insert(j[k]);
                        }
                    }
                }
            }
        }
        let joint_nodes: Vec<u16> = used.into_iter().collect();
        let mut remap = vec![0u16; actor.nodes.len().max(1)];
        for (compact, &node) in joint_nodes.iter().enumerate() {
            remap[node as usize] = compact as u16;
        }
        let inverse_bindposes = stores.bindposes.add(
            joint_nodes
                .iter()
                .map(|&n| rig.bind_world[n as usize].inverse())
                .collect::<Vec<_>>(),
        );
        let mut meshes = Vec::new();
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        let mut sub_materials: HashMap<u8, Handle<StandardMaterial>> = HashMap::new();
        for mesh in &actor.meshes {
            for sub in &mesh.submeshes {
                for p in &sub.positions {
                    let p = Vec3::new(-p[0], p[1], p[2]);
                    min = min.min(p);
                    max = max.max(p);
                }
                let material = match sub_materials.get(&sub.material) {
                    Some(h) => h.clone(),
                    None => {
                        let h = if sub.material == 0
                            || sub.material as usize >= actor.materials.len()
                        {
                            self.shared_material(None, None, stores)
                        } else {
                            self.actor_material(&actor, sub.material as usize, &path, skin, stores)
                        };
                        sub_materials.insert(sub.material, h.clone());
                        h
                    }
                };
                let _p = Prof::start(4);
                meshes.push(PartMesh {
                    mesh: stores.meshes.add(skinned_mesh(sub, &remap)),
                    material,
                });
            }
        }
        if !min.is_finite() {
            min = Vec3::ZERO;
            max = Vec3::ZERO;
        }
        // Generous bounds: poses (jumps, attacks, falls) leave the bind-pose box.
        let pad = (max - min) * 0.35 + Vec3::splat(20.0);
        let part = Arc::new(PartAsset {
            name: match skin {
                Some(n) => format!("{}@M{n}", path.trim_end_matches(".xact")),
                None => path.trim_end_matches(".xact").to_string(),
            },
            rig,
            joint_nodes,
            inverse_bindposes,
            meshes,
            aabb: Aabb::from_min_max(min - pad, max + pad),
        });
        self.parts.insert(key, part.clone());
        Ok(part)
    }

    /// Builds (or returns the cached) composition of a body skeleton and extra parts. The
    /// first part is the body; every other part is bound to the body's joints by node name.
    pub fn composition(
        &mut self,
        part_names: &[&str],
        stores: &mut AssetStores,
    ) -> Result<Arc<Composition>> {
        let mut parts = Vec::new();
        for name in part_names {
            parts.push(self.part(name, stores)?);
        }
        let key = parts
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>()
            .join("+");
        if let Some(c) = self.compositions.get(&key) {
            return Ok(c.clone());
        }
        let rig = parts
            .first()
            .ok_or_else(|| anyhow!("composition needs a body part"))?
            .rig
            .clone();
        let mut master: BTreeSet<u16> = BTreeSet::new();
        // Master node of each part joint.
        let mut mapped: Vec<Vec<u16>> = Vec::new();
        for part in &parts {
            let nodes = part
                .joint_nodes
                .iter()
                .map(|&n| map_node(&rig, &part.rig, n))
                .collect::<Vec<_>>();
            master.extend(nodes.iter().copied());
            mapped.push(nodes);
        }
        // Equipment slots are always available as joints.
        for (i, n) in rig.names.iter().enumerate() {
            if n.starts_with("Slot_") {
                master.insert(i as u16);
            }
        }
        // Slots that only extra parts define hang off the nearest shared joint at their bind
        // offset.
        let mut part_slots: Vec<(String, u16, Affine3A)> = Vec::new();
        for part in parts.iter().skip(1) {
            for (i, name) in part.rig.names.iter().enumerate() {
                if !name.starts_with("Slot_") || rig.index.contains_key(&name.to_lowercase()) {
                    continue;
                }
                let m = map_node(&rig, &part.rig, i as u16);
                master.insert(m);
                let offset = rig.bind_world[m as usize].inverse() * part.rig.bind_world[i];
                part_slots.push((name.to_lowercase(), m, Affine3A::from_mat4(offset)));
            }
        }
        let joint_nodes: Vec<u16> = master.into_iter().collect();
        let position: HashMap<u16, u16> = joint_nodes
            .iter()
            .enumerate()
            .map(|(i, &n)| (n, i as u16))
            .collect();
        let part_joints = mapped
            .iter()
            .map(|nodes| nodes.iter().map(|n| position[n]).collect())
            .collect();
        let plan: Arc<[crate::rig::EvalStep]> = rig.plan(&joint_nodes).into();
        let joint_by_name = joint_nodes
            .iter()
            .enumerate()
            .map(|(i, &n)| (rig.names[n as usize].to_lowercase(), i as u16))
            .collect();
        let slot_mounts = part_slots
            .into_iter()
            .map(|(name, m, offset)| (name, (position[&m], offset)))
            .collect();
        let facing_fix = self.facing_fix(&rig, &plan);
        let composition = Arc::new(Composition {
            key: key.clone(),
            rig,
            parts,
            joint_nodes: joint_nodes.into(),
            part_joints,
            plan,
            facing_fix,
            joint_by_name,
            slot_mounts,
        });
        self.compositions.insert(key, composition.clone());
        Ok(composition)
    }

    /// Facing correction measured on the family's idle clip (identity when it cannot be
    /// determined).
    fn facing_fix(&mut self, rig: &Arc<Rig>, plan: &[crate::rig::EvalStep]) -> Quat {
        let query = crate::names::MotionQuery {
            pose: Some("Stand"),
            left: Some("None"),
            right: Some("None"),
            pn: Some("P0"),
            action: Some("Ambient"),
            phase: Some("Loop"),
            ..Default::default()
        };
        let Some(name) = self.catalog.find(&rig.family, &query).map(str::to_string) else {
            return Quat::IDENTITY;
        };
        let Ok(motion) = self.motion(rig, &name) else {
            return Quat::IDENTITY;
        };
        let mut pose = rig.rest.clone();
        motion
            .binding
            .sample(&motion.clip, &rig.rest, 0.0, &mut pose);
        let mut model = vec![Affine3A::IDENTITY; rig.names.len()];
        crate::rig::evaluate(plan, &pose, &mut model);
        match rig.facing_in_pose(&model) {
            Some(facing) => Quat::from_rotation_arc(facing, crate::locomotion::FORWARD),
            None => Quat::IDENTITY,
        }
    }

    // ---- motions ------------------------------------------------------------------------

    /// Loads (or returns the cached) clip by exact name (without extension).
    pub fn clip(&mut self, name: &str) -> Result<Arc<Clip>> {
        let key = name.to_lowercase().trim_end_matches(".xmot").to_string();
        if let Some(c) = self.clips.get(&key) {
            return Ok(c.clone());
        }
        let at = *self
            .xmot_index
            .get(&format!("{key}.xmot"))
            .ok_or_else(|| anyhow!("no .xmot motion named {name:?}"))?;
        let _p = Prof::start(3);
        let path = self.animation.entries[at].path.clone();
        let raw = RawMotion::parse(&self.animation.read_payload(at)?)
            .map_err(|e| e.context(format!("parsing {path}")))?;
        let stem = path.trim_end_matches(".xmot");
        let mut clip = Clip::from_motion(stem, &raw);
        if let Some(parsed) = MotionName::parse(stem) {
            clip.looping = parsed.is_cyclic();
            if parsed.action == "Move" {
                use crate::locomotion::{FORWARD, RIGHT};
                match parsed.phase.as_str() {
                    "Walk" | "Run" | "Sprint" => {
                        clip.implied_dir = match parsed.dir.as_str() {
                            "Back" => -FORWARD,
                            "Left" => -RIGHT,
                            "Right" => RIGHT,
                            _ => FORWARD,
                        };
                    }
                    "Turn90" => {
                        let quarter = std::f32::consts::FRAC_PI_2;
                        clip.implied_turn = if parsed.dir == "Left" {
                            quarter
                        } else {
                            -quarter
                        };
                    }
                    _ => {}
                }
            }
        }
        let clip = Arc::new(clip);
        self.clips.insert(key, clip.clone());
        Ok(clip)
    }

    /// A clip bound to `rig` (binding cached per rig and clip).
    pub fn motion(&mut self, rig: &Arc<Rig>, name: &str) -> Result<Motion> {
        let clip = self.clip(name)?;
        let key = (rig.name.to_lowercase(), clip.name.to_lowercase());
        let binding = match self.bindings.get(&key) {
            Some(b) => b.clone(),
            None => {
                let b = Arc::new(ClipBinding::new(rig, &clip));
                self.bindings.insert(key, b.clone());
                b
            }
        };
        Ok(Motion { clip, binding })
    }

    /// Motion name for a locomotion key in the rig's skeleton family (cached).
    pub fn locomotion_motion(&mut self, family: &str, key: LocoKey) -> Option<String> {
        let cache_key = (family.to_lowercase(), key);
        if let Some(found) = self.loco.get(&cache_key) {
            return found.clone();
        }
        let found = resolve_motion(&self.catalog, family, key).map(str::to_string);
        self.loco.insert(cache_key, found.clone());
        found
    }

    // ---- static meshes ------------------------------------------------------------------

    /// Loads a static `.xcmsh` (weapons, tools) with its diffuse textures.
    pub fn static_mesh(
        &mut self,
        name: &str,
        stores: &mut AssetStores,
    ) -> Result<Arc<StaticAsset>> {
        self.static_mesh_variant(name, None, stores)
    }

    /// Like [`Self::static_mesh`] with a `MaterialSwitch` applied to the diffuse textures
    /// (hair and beards).
    pub fn static_mesh_variant(
        &mut self,
        name: &str,
        switch: Option<i32>,
        stores: &mut AssetStores,
    ) -> Result<Arc<StaticAsset>> {
        let base = name.to_lowercase().trim_end_matches(".xcmsh").to_string();
        let key = match switch {
            Some(n) => format!("{base}@M{n}"),
            None => base.clone(),
        };
        let name = base.as_str();
        if let Some(s) = self.statics.get(&key) {
            return Ok(s.clone());
        }
        let key_mesh = base.clone();
        let at = *self
            .mesh_index
            .get(&format!("{key_mesh}.xcmsh"))
            .ok_or_else(|| anyhow!("no .xcmsh mesh named {name:?}"))?;
        let bytes = self.mesh_pak.read_payload(at)?;
        let xcmsh = Xcmsh::parse(&bytes).with_context(|| format!("parsing {name}"))?;
        let mut meshes = Vec::new();
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for sub in &xcmsh.submeshes {
            // Mirrored into engine space like the actors (x -> -x, winding reversed by the
            // reflection, so no index swap).
            let (bmin, bmax) = (sub.bounds_min, sub.bounds_max);
            min = min.min(Vec3::new(-bmax[0], bmin[1], bmin[2]));
            max = max.max(Vec3::new(-bmin[0], bmax[1], bmax[2]));
            let indices = sub.indices.clone();
            let mirror_v = |v: &[f32; 3]| [-v[0], v[1], v[2]];
            let positions: Vec<[f32; 3]> = sub.positions.iter().map(mirror_v).collect();
            let normals: Vec<[f32; 3]> = sub.normals.iter().map(mirror_v).collect();
            let mut mesh = Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::RENDER_WORLD,
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
            .with_inserted_indices(Indices::U32(indices));
            if sub.normals.len() == sub.positions.len() {
                mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
            } else {
                mesh.compute_flat_normals();
            }
            if sub.uvs.len() == sub.positions.len() {
                mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, sub.uvs.clone());
            }
            let file = if sub.material.to_lowercase().ends_with(".xshmat") {
                sub.material.clone()
            } else {
                format!("{}.xshmat", sub.material)
            };
            let (mut stem, blend) = self.material_file(&file).unwrap_or((None, None));
            if let (Some(base), Some(sw)) = (stem.as_deref(), switch) {
                stem = Some(self.switch_stem(base, sw));
            }
            let material = self.shared_material(stem, blend, stores);
            meshes.push(PartMesh {
                mesh: stores.meshes.add(mesh),
                material,
            });
        }
        let asset = Arc::new(StaticAsset {
            name: name.to_string(),
            meshes,
            bounds_min: if min.is_finite() { min } else { Vec3::ZERO },
            bounds_max: if max.is_finite() { max } else { Vec3::ZERO },
        });
        self.statics.insert(key, asset.clone());
        Ok(asset)
    }

    /// Names of static meshes containing `needle` (case-insensitive), sorted.
    pub fn static_mesh_names(&self, needle: &str) -> Vec<String> {
        let needle = needle.to_lowercase();
        let mut v: Vec<String> = self
            .mesh_index
            .keys()
            .filter(|k| k.contains(&needle))
            .map(|k| k.trim_end_matches(".xcmsh").to_string())
            .collect();
        v.sort();
        v
    }
}

/// Master node for a part node: same name in the master, else the nearest ancestor (in the
/// part's own hierarchy) that exists there, else the master's first animated node.
fn map_node(master: &Rig, part: &Rig, node: u16) -> u16 {
    let mut n = node;
    while n != NONE {
        if let Some(&m) = master.index.get(&part.names[n as usize].to_lowercase()) {
            return m;
        }
        n = part.parent[n as usize];
    }
    master.slot_node.first().copied().unwrap_or(0)
}

fn alpha_mode(blend: Option<(u32, u8)>) -> AlphaMode {
    match blend {
        Some((1, mask)) => AlphaMode::Mask(f32::from(mask) / 255.0),
        Some((2 | 8, _)) => AlphaMode::Blend,
        Some((7, _)) => AlphaMode::Add,
        _ => AlphaMode::Opaque,
    }
}

fn skinned_mesh(sub: &ActorSubmesh, remap: &[u16]) -> Mesh {
    // Mirrored into engine space (x -> -x); the reflection also reverses the winding, which
    // replaces the index swap the unmirrored data needed.
    let indices = sub.indices.clone();
    let mirror_v = |v: &[f32; 3]| [-v[0], v[1], v[2]];
    let positions: Vec<[f32; 3]> = sub.positions.iter().map(mirror_v).collect();
    let normals: Vec<[f32; 3]> = sub.normals.iter().map(mirror_v).collect();
    let joints: Vec<[u16; 4]> = sub
        .joints
        .iter()
        .zip(&sub.weights)
        .map(|(j, w)| {
            let mut out = [0u16; 4];
            for k in 0..4 {
                out[k] = if w[k] != 0.0 { remap[j[k] as usize] } else { 0 };
            }
            out
        })
        .collect();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, sub.uvs.clone())
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_JOINT_INDEX,
        VertexAttributeValues::Uint16x4(joints),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, sub.weights.clone())
    .with_inserted_indices(Indices::U32(indices))
}

/// Replaces the geometry of a parsed actor with a remaster override: same skeleton and
/// materials, joints matched by node name, materials by the `#<index>` prefix.
fn apply_remaster(actor: &mut Actor, ov: &gothic_formats::remaster::OverrideMesh) {
    if !ov.skinned {
        return;
    }
    let by_name: HashMap<String, u16> = actor
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.name.to_lowercase(), i as u16))
        .collect();
    let node_map: Vec<u16> = ov
        .nodes
        .iter()
        .map(|n| by_name.get(&n.to_lowercase()).copied().unwrap_or(0))
        .collect();
    let mut submeshes = Vec::new();
    let mut total = 0u32;
    for s in &ov.submeshes {
        let material = s
            .material
            .strip_prefix('#')
            .and_then(|m| m.split(':').next())
            .and_then(|m| m.parse::<u8>().ok())
            .filter(|&m| (m as usize) < actor.materials.len())
            .or_else(|| {
                actor
                    .materials
                    .iter()
                    .position(|m| m.name == s.material)
                    .map(|i| i as u8)
            })
            .unwrap_or(0);
        let n = s.positions.len() as u32;
        submeshes.push(ActorSubmesh {
            material,
            positions: s.positions.clone(),
            normals: s.normals.clone(),
            uvs: s.uvs.clone(),
            uvs2: Vec::new(),
            indices: s.lods.first().cloned().unwrap_or_default(),
            original_vertex: (total..total + n).collect(),
            joints: s
                .joints
                .iter()
                .map(|j| j.map(|k| node_map.get(k as usize).copied().unwrap_or(0)))
                .collect(),
            weights: s.weights.clone(),
        });
        total += n;
    }
    let node = actor.meshes.first().map_or(0, |m| m.node);
    actor.meshes = vec![gothic_formats::xact::ActorMesh {
        node,
        original_vertex_count: total,
        skinned: true,
        max_influences: 4,
        submeshes,
    }];
}

fn texture_image(image: &Ximg) -> Option<Image> {
    let mips = image.mips();
    let data: Vec<u8> = mips.iter().flat_map(|m| m.iter().copied()).collect();
    bc_image(
        image.width,
        image.height,
        &image.fourcc,
        data,
        mips.len() as u32,
    )
}

fn bc_image(
    width: u32,
    height: u32,
    fourcc: &[u8; 4],
    data: Vec<u8>,
    mip_count: u32,
) -> Option<Image> {
    let format = match fourcc {
        b"DXT1" => TextureFormat::Bc1RgbaUnormSrgb,
        b"DXT3" => TextureFormat::Bc2RgbaUnormSrgb,
        b"DXT5" => TextureFormat::Bc3RgbaUnormSrgb,
        _ => return None,
    };
    if !width.is_multiple_of(4) || !height.is_multiple_of(4) {
        return None;
    }
    let mut out = Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    out.texture_descriptor.mip_level_count = mip_count;
    out.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..Default::default()
    });
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_names_normalise() {
        assert_eq!(basename("a/b/C.XACT"), "c.xact");
    }
}
