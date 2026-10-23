//! [`LibrarySource`]: the real [`BuildSource`] over the game archives.
//!
//! It runs the CPU stage of [`ActorLibrary`] (the same functions the blocking
//! [`ActorLibrary::composition`] / [`crate::ActorSpawner::spawn`] path uses: file reads, the
//! mirrored engine-space skeleton and meshes, joint tables, the evaluation plan, materials,
//! texture decode, clip binding) and returns an [`ActorBundle`] instead of filling `Assets`.
//! A committed character therefore has the same skeleton, meshes, materials, attachments and
//! starting motion as one spawned by `ActorSpawner::spawn` from the equivalent
//! [`NpcSpec`] (see [`BuildRequest::from_spec`]).
//!
//! Every worker needs its own archive handles (reads seek a shared file cursor), so the source
//! keeps a small pool of libraries and opens another one when all are busy: at most one per
//! worker thread. Each pooled library keeps its own clip, binding and catalog caches.
use crate::library::{ActorLibrary, split_switch};
use crate::locomotion::{LocoKey, Stance};
use crate::runtime::NpcSpec;
use crate::stream::{
    ActorBundle, AttachRequest, BuildRequest, BuildSource, CpuAttachment, CpuMesh, CpuPart,
};
use anyhow::{Context, Result, anyhow, bail};
use bevy::math::Affine3A;
use bevy::prelude::Transform;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

type Opener = dyn Fn() -> Result<ActorLibrary> + Send + Sync;

/// Builds characters from the installed archives on the streamer's worker threads.
pub struct LibrarySource {
    open: Box<Opener>,
    pool: Mutex<Vec<ActorLibrary>>,
    opened: AtomicUsize,
}

impl LibrarySource {
    /// `open` makes one library (archives plus any clip packs the caller registers on it).
    /// It is called lazily, from a worker thread, whenever every pooled library is busy.
    pub fn new(open: impl Fn() -> Result<ActorLibrary> + Send + Sync + 'static) -> Self {
        Self {
            open: Box::new(open),
            pool: Mutex::new(Vec::new()),
            opened: AtomicUsize::new(0),
        }
    }

    /// Opens the archives under `data_dir` (as [`ActorLibrary::open`]) for every pooled
    /// library.
    pub fn open(data_dir: impl Into<PathBuf>) -> Self {
        let data_dir = data_dir.into();
        Self::new(move || ActorLibrary::open(&data_dir))
    }

    /// Adds an already opened library to the pool.
    pub fn with_library(self, library: ActorLibrary) -> Self {
        self.pool
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(library);
        self
    }

    /// Libraries opened by the source so far (not counting [`Self::with_library`]).
    pub fn opened(&self) -> usize {
        self.opened.load(Ordering::Relaxed)
    }

    fn take(&self) -> Result<ActorLibrary> {
        let pooled = self.pool.lock().unwrap_or_else(|p| p.into_inner()).pop();
        match pooled {
            Some(library) => Ok(library),
            None => {
                let library = (self.open)().context("opening the actor archives")?;
                self.opened.fetch_add(1, Ordering::Relaxed);
                Ok(library)
            }
        }
    }
}

impl BuildSource for LibrarySource {
    fn build(&self, request: &BuildRequest) -> Result<ActorBundle> {
        let mut library = self.take()?;
        // A panic inside drops this library; the next build opens a fresh one.
        let result = library.build_bundle(request);
        self.pool
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(library);
        result
    }
}

impl BuildRequest {
    /// The request that builds what `ActorSpawner::spawn(spec, ..)` spawns: the parts, the
    /// weapons (drawn and holster slot) then the slot attachments (`mesh@M<switch>`), the
    /// scripted motion and the start time. The stance is passed to
    /// [`crate::ActorStreamer::request`] separately. Weapon offsets other than identity are
    /// not supported by the streamer and are refused.
    pub fn from_spec(spec: &NpcSpec) -> Result<Self> {
        let mut attachments = Vec::with_capacity(spec.weapons.len() + spec.attachments.len());
        for w in &spec.weapons {
            if w.offset != Transform::IDENTITY {
                bail!("weapon {}: offset attachments cannot be streamed", w.mesh);
            }
            attachments.push(AttachRequest {
                mesh: w.mesh.clone(),
                slot: w.drawn_slot.clone(),
                holster_slot: Some(w.holster_slot.clone()),
            });
        }
        for a in &spec.attachments {
            // A negative switch keeps the authored texture, like no switch at all.
            let mesh = if a.switch >= 0 {
                format!("{}@M{}", a.mesh, a.switch)
            } else {
                a.mesh.clone()
            };
            attachments.push(AttachRequest {
                mesh,
                slot: a.slot.clone(),
                holster_slot: None,
            });
        }
        Ok(Self {
            parts: spec.parts.clone(),
            attachments,
            motion: spec.motion.clone(),
            start_time: spec.start_time,
        })
    }
}

impl ActorLibrary {
    /// CPU stage of a whole character: the bundle a [`LibrarySource`] returns. Shares every
    /// loading step with [`ActorLibrary::composition`] and `ActorSpawner::spawn`; only the
    /// final move into `Assets` differs (done by the streamer's commit).
    pub fn build_bundle(&mut self, request: &BuildRequest) -> Result<ActorBundle> {
        if request.parts.is_empty() {
            bail!("composition needs a body part");
        }
        let mut builds = Vec::with_capacity(request.parts.len());
        for name in &request.parts {
            builds.push(self.build_part_named(name)?);
        }
        let skeleton = self.skeleton(builds.iter().map(|b| (&b.rig, b.joint_nodes.as_slice())))?;
        let mut materials = HashMap::new();
        let mut parts = Vec::with_capacity(builds.len());
        for (b, joints) in builds.into_iter().zip(&skeleton.part_joints) {
            let meshes = b
                .meshes
                .into_iter()
                .map(|(mesh, info)| CpuMesh {
                    mesh,
                    material: self.cpu_material(info, &mut materials),
                })
                .collect();
            parts.push(CpuPart {
                name: b.name,
                rig: b.rig,
                joints: joints.clone(),
                joint_rig_nodes: b.joint_nodes,
                inverse_bindposes: b.inverse_bindposes,
                meshes,
                aabb: b.aabb,
            });
        }
        let joint_of = |slot: &str| -> Result<u16> {
            skeleton
                .joint_by_name
                .get(&slot.to_lowercase())
                .copied()
                .ok_or_else(|| anyhow!("rig has no slot {slot:?}"))
        };
        let mut attachments = Vec::with_capacity(request.attachments.len());
        for a in &request.attachments {
            let (mesh, switch) = split_switch(&a.mesh);
            let base = mesh.to_lowercase().trim_end_matches(".xcmsh").to_string();
            let built = self.build_static(&base, switch)?;
            let (joint, offset, weapon) = match &a.holster_slot {
                // A weapon: drawn slot required, holster falls back to it.
                Some(holster) => {
                    let drawn = joint_of(&a.slot)?;
                    let holster = joint_of(holster).unwrap_or(drawn);
                    (drawn, Affine3A::IDENTITY, Some((drawn, holster)))
                }
                None => {
                    let lower = a.slot.to_lowercase();
                    let (joint, offset) = match skeleton.joint_by_name.get(&lower) {
                        Some(&j) => (j, Affine3A::IDENTITY),
                        None => skeleton
                            .slot_mounts
                            .get(&lower)
                            .copied()
                            .ok_or_else(|| anyhow!("no part defines slot {:?}", a.slot))?,
                    };
                    (joint, offset, None)
                }
            };
            let meshes = built
                .meshes
                .into_iter()
                .map(|(mesh, info)| CpuMesh {
                    mesh,
                    material: self.cpu_material(info, &mut materials),
                })
                .collect();
            attachments.push(CpuAttachment {
                meshes,
                joint,
                offset,
                weapon,
            });
        }
        let rig = skeleton.rig.clone();
        let (motion, idle) = match &request.motion {
            Some(name) => (Some(self.motion(&rig, name)?), Vec::new()),
            None => {
                let mut idle = Vec::new();
                for stance in Stance::ALL {
                    let Some(name) = self.locomotion_motion(&rig.family, LocoKey::idle(stance))
                    else {
                        continue;
                    };
                    match self.motion(&rig, &name) {
                        Ok(m) => idle.push((stance, m)),
                        Err(e) => eprintln!("animation: idle clip {name}: {e:#}"),
                    }
                }
                (None, idle)
            }
        };
        Ok(ActorBundle {
            key: request.key(),
            rig,
            plan: skeleton.plan,
            joint_nodes: skeleton.joint_nodes,
            facing_fix: skeleton.facing_fix,
            joint_by_name: skeleton.joint_by_name,
            slot_mounts: skeleton.slot_mounts,
            parts,
            attachments,
            locomotion: request.motion.is_none(),
            motion,
            idle,
        })
    }
}
