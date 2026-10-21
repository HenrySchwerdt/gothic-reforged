//! Asynchronous character building: the expensive part of spawning a character (reading and
//! parsing files, building skinned mesh buffers, decoding textures, joint tables, the
//! evaluation plan) runs on worker threads and produces a CPU-only [`ActorBundle`]; the main
//! thread only *commits* it (moves the buffers into `Assets`, spawns the entities), under a
//! per-frame time budget and nearest-first.
//!
//! ```text
//! request()  --> queue (priority: distance to StreamFocus) --> worker: BuildSource::build
//!   |                                                               |
//!   +-> placeholder root entity (PendingActor)        ActorBundle --+
//!                                                                   v
//! commit system (Update, budget): assets once per look, entities per request
//! ```
//!
//! A committed look is cached: the next character with the same parts skips the workers and
//! costs only the entity spawn. The committed character has exactly the components of
//! [`crate::ActorSpawner::spawn`] (`Animator`, `CharacterComposition`, `CharacterJoints`,
//! `PartMeshes`, skinned meshes, attachments), so every other system works unchanged.
use crate::animator::{Animator, Motion};
use crate::library::{Composition, PartAsset, PartMesh};
use crate::locomotion::{Locomotion, LocomotionInput, Stance};
use crate::rig::{EvalStep, Rig};
use crate::runtime::{
    Attachment, CharacterComposition, CharacterJoints, PartMeshes, SkeletonJoint, WeaponMount,
};
use anyhow::Result;
use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::{NoFrustumCulling, Visibility};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Query, ResMut};
use bevy::math::{Affine3A, Mat4, Quat, Vec3};
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::mesh::{Mesh, Mesh3d};
use bevy::prelude::{
    Assets, GlobalTransform, Handle, Image, MeshMaterial3d, StandardMaterial, Transform,
};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// What to build: the character's parts (body first) and its look-independent options.
#[derive(Debug, Clone, PartialEq)]
pub struct BuildRequest {
    /// Part names, body first (`Hero_Body_Naked`, `Hero_Head_01`, ...).
    pub parts: Vec<String>,
    /// Weapons and attachments by mesh name and slot, resolved by the source.
    pub attachments: Vec<AttachRequest>,
    /// Motion to play instead of the idle locomotion.
    pub motion: Option<String>,
}

impl BuildRequest {
    pub fn new(parts: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            parts: parts.into_iter().map(Into::into).collect(),
            attachments: Vec::new(),
            motion: None,
        }
    }

    /// Cache key: equal keys share one build and one set of assets.
    pub fn key(&self) -> String {
        let mut k = self.parts.join("+");
        for a in &self.attachments {
            k.push_str(&format!("|{}@{}", a.mesh, a.slot));
        }
        if let Some(m) = &self.motion {
            k.push_str(&format!("|{m}"));
        }
        k
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AttachRequest {
    pub mesh: String,
    pub slot: String,
    /// Holster slot for weapons (switches with the stance), `None` for fixed attachments.
    pub holster_slot: Option<String>,
}

/// A material and its texture, not yet in `Assets`.
#[derive(Clone)]
pub struct CpuMaterial {
    /// `base_color_texture` is filled in at commit time.
    pub material: StandardMaterial,
    /// Texture key (shared between looks) and image; `None` for untextured materials.
    pub texture: Option<(String, Arc<Image>)>,
}

pub struct CpuMesh {
    pub mesh: Mesh,
    pub material: CpuMaterial,
}

pub struct CpuPart {
    pub name: String,
    /// Index into [`ActorBundle::joint_nodes`] for every skin joint of this part.
    pub joints: Vec<u16>,
    /// Rig node of every skin joint (for [`PartAsset::joint_nodes`]).
    pub joint_rig_nodes: Vec<u16>,
    pub inverse_bindposes: Vec<Mat4>,
    pub meshes: Vec<CpuMesh>,
    pub aabb: Aabb,
}

pub struct CpuAttachment {
    pub meshes: Vec<CpuMesh>,
    pub joint: u16,
    pub offset: Affine3A,
    /// `(drawn, holster)` joints for weapons.
    pub weapon: Option<(u16, u16)>,
}

/// Everything needed to spawn a character, built off the main thread.
pub struct ActorBundle {
    pub key: String,
    pub rig: Arc<Rig>,
    pub plan: Arc<[EvalStep]>,
    pub joint_nodes: Arc<[u16]>,
    pub facing_fix: Quat,
    pub joint_by_name: HashMap<String, u16>,
    pub parts: Vec<CpuPart>,
    pub attachments: Vec<CpuAttachment>,
    /// Motion to start with (idle or the requested one).
    pub motion: Option<Motion>,
    /// Use the locomotion state machine (no explicit motion was requested).
    pub locomotion: bool,
}

/// Builds bundles on worker threads. Implementations must not touch the ECS.
pub trait BuildSource: Send + Sync + 'static {
    fn build(&self, request: &BuildRequest) -> Result<ActorBundle>;
}

/// Placeholder root of a requested character; removed when it is committed.
#[derive(Component, Debug, Clone)]
pub struct PendingActor {
    pub key: String,
}

/// Distances for priorities are measured from this entity (camera or hero).
#[derive(Component)]
pub struct StreamFocus;

/// Per-frame and cumulative numbers.
#[derive(Debug, Default, Clone)]
pub struct StreamStats {
    pub queued: usize,
    pub building: usize,
    pub pending_spawns: usize,
    pub built: usize,
    pub failed: usize,
    pub committed_looks: usize,
    pub spawned: usize,
    pub spawned_this_frame: usize,
    /// Main-thread time spent committing this frame.
    pub commit_this_frame: Duration,
    pub max_commit_frame: Duration,
    /// Longest single character commit (assets + entities).
    pub max_commit_one: Duration,
    /// Worker time of all builds.
    pub build_total: Duration,
    pub max_build: Duration,
}

struct Job {
    key: String,
    request: BuildRequest,
    position: Vec3,
}

struct Shared {
    jobs: Mutex<Vec<Job>>,
    wake: Condvar,
    focus: Mutex<Vec3>,
    stop: AtomicBool,
}

type Built = (String, Result<ActorBundle>, Duration);

struct PendingSpawn {
    entity: Entity,
    key: String,
    position: Vec3,
    stance: Stance,
}

/// A committed attachment: meshes, joint, offset, (hand, holster) joints of a weapon.
type CommittedAttachment = (Vec<PartMesh>, u16, Affine3A, Option<(u16, u16)>);

/// Committed look: assets in `Assets`, shareable by every character with that look.
struct Committed {
    composition: Arc<Composition>,
    attachments: Vec<CommittedAttachment>,
    motion: Option<Motion>,
    locomotion: bool,
}

/// Settings of the streamer.
#[derive(Debug, Clone)]
pub struct StreamConfig {
    pub workers: usize,
    /// Main-thread time the commit system may spend per frame (at least one character is
    /// committed per frame while any is ready).
    pub frame_budget: Duration,
    /// Characters committed per frame at most.
    pub max_per_frame: usize,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            workers: std::thread::available_parallelism()
                .map_or(2, |n| n.get().saturating_sub(1))
                .clamp(1, 4),
            frame_budget: Duration::from_micros(1500),
            max_per_frame: 8,
        }
    }
}

/// The streamer resource: request characters, the commit system spawns them.
#[derive(Resource)]
pub struct ActorStreamer {
    source: Arc<dyn BuildSource>,
    shared: Arc<Shared>,
    results: Mutex<Receiver<Built>>,
    threads: Vec<std::thread::JoinHandle<()>>,
    config: StreamConfig,
    in_flight: HashSet<String>,
    ready: HashMap<String, ActorBundle>,
    committed: HashMap<String, Arc<Committed>>,
    failed: HashMap<String, String>,
    pending: Vec<PendingSpawn>,
    textures: HashMap<String, Handle<Image>>,
    pub stats: StreamStats,
}

impl ActorStreamer {
    pub fn new(source: Arc<dyn BuildSource>, config: StreamConfig) -> Self {
        let shared = Arc::new(Shared {
            jobs: Mutex::new(Vec::new()),
            wake: Condvar::new(),
            focus: Mutex::new(Vec3::ZERO),
            stop: AtomicBool::new(false),
        });
        let (tx, rx) = channel::<Built>();
        let threads = (0..config.workers.max(1))
            .map(|i| {
                let shared = shared.clone();
                let source: Arc<dyn BuildSource> = source.clone();
                let tx: Sender<Built> = tx.clone();
                std::thread::Builder::new()
                    .name(format!("actor-build-{i}"))
                    .spawn(move || worker(&shared, source.as_ref(), &tx))
                    .expect("spawn actor build worker")
            })
            .collect();
        Self {
            source,
            shared,
            results: Mutex::new(rx),
            threads,
            config,
            in_flight: HashSet::new(),
            ready: HashMap::new(),
            committed: HashMap::new(),
            failed: HashMap::new(),
            pending: Vec::new(),
            textures: HashMap::new(),
            stats: StreamStats::default(),
        }
    }

    pub fn config(&self) -> &StreamConfig {
        &self.config
    }

    /// Request a character at `transform`. Returns its root entity at once (a placeholder
    /// with [`PendingActor`]); the skeleton, meshes and animator are added when it is
    /// committed. Despawning the root before that cancels the spawn.
    pub fn request(
        &mut self,
        commands: &mut Commands,
        request: BuildRequest,
        transform: Transform,
        stance: Stance,
    ) -> Entity {
        let key = request.key();
        let entity = commands
            .spawn((
                transform,
                Visibility::default(),
                PendingActor { key: key.clone() },
            ))
            .id();
        let position = transform.translation;
        if !self.committed.contains_key(&key)
            && !self.ready.contains_key(&key)
            && !self.failed.contains_key(&key)
            && self.in_flight.insert(key.clone())
        {
            let mut jobs = self.shared.jobs.lock().unwrap_or_else(|p| p.into_inner());
            jobs.push(Job {
                key: key.clone(),
                request,
                position,
            });
            self.shared.wake.notify_one();
        }
        self.pending.push(PendingSpawn {
            entity,
            key,
            position,
            stance,
        });
        entity
    }

    /// Build (if needed) and spawn a character right now on the calling thread, ignoring the
    /// budget: for loading screens and tests. This is what spawning cost before streaming.
    pub fn spawn_blocking(
        &mut self,
        commands: &mut Commands,
        assets: &mut StreamAssets,
        request: BuildRequest,
        transform: Transform,
        stance: Stance,
    ) -> Result<Entity> {
        let key = request.key();
        let look = match self.committed.get(&key) {
            Some(c) => c.clone(),
            None => {
                let start = Instant::now();
                let bundle = self.source.build(&request)?;
                let took = start.elapsed();
                self.stats.built += 1;
                self.stats.build_total += took;
                self.stats.max_build = self.stats.max_build.max(took);
                self.commit_assets(bundle, assets)
            }
        };
        let entity = commands
            .spawn((transform, Visibility::default(), PendingActor { key }))
            .id();
        spawn_character(commands, entity, &look, stance);
        self.stats.spawned += 1;
        Ok(entity)
    }

    /// Why a look failed to build (the pending characters of it are dropped).
    pub fn failure(&self, key: &str) -> Option<&str> {
        self.failed.get(key).map(String::as_str)
    }

    pub fn is_idle(&self) -> bool {
        self.pending.is_empty() && self.in_flight.is_empty()
    }

    fn set_focus(&self, focus: Vec3) {
        *self.shared.focus.lock().unwrap_or_else(|p| p.into_inner()) = focus;
    }

    fn collect(&mut self) {
        let results: Vec<Built> = {
            let rx = self.results.lock().unwrap_or_else(|p| p.into_inner());
            rx.try_iter().collect()
        };
        for (key, result, took) in results {
            self.in_flight.remove(&key);
            self.stats.build_total += took;
            self.stats.max_build = self.stats.max_build.max(took);
            match result {
                Ok(bundle) => {
                    self.stats.built += 1;
                    self.ready.insert(key, bundle);
                }
                Err(e) => {
                    self.stats.failed += 1;
                    self.failed.insert(key, format!("{e:#}"));
                }
            }
        }
    }

    /// Move a bundle's buffers into `Assets` (once per look).
    fn commit_assets(&mut self, bundle: ActorBundle, assets: &mut StreamAssets) -> Arc<Committed> {
        let mut add_meshes =
            |meshes: Vec<CpuMesh>, textures: &mut HashMap<String, Handle<Image>>| {
                meshes
                    .into_iter()
                    .map(|m| {
                        let mut material = m.material.material;
                        if let Some((key, image)) = m.material.texture {
                            let handle = textures
                                .entry(key)
                                .or_insert_with(|| {
                                    assets.images.add(
                                        Arc::try_unwrap(image).unwrap_or_else(|a| (*a).clone()),
                                    )
                                })
                                .clone();
                            material.base_color_texture = Some(handle);
                        }
                        PartMesh {
                            mesh: assets.meshes.add(m.mesh),
                            material: assets.materials.add(material),
                        }
                    })
                    .collect::<Vec<_>>()
            };
        let mut parts = Vec::with_capacity(bundle.parts.len());
        let mut part_joints = Vec::with_capacity(bundle.parts.len());
        for p in bundle.parts {
            let inverse_bindposes = assets
                .bindposes
                .add(SkinnedMeshInverseBindposes::from(p.inverse_bindposes));
            let meshes = add_meshes(p.meshes, &mut self.textures);
            part_joints.push(p.joints);
            parts.push(Arc::new(PartAsset {
                name: p.name,
                rig: bundle.rig.clone(),
                joint_nodes: p.joint_rig_nodes,
                inverse_bindposes,
                meshes,
                aabb: p.aabb,
            }));
        }
        let attachments = bundle
            .attachments
            .into_iter()
            .map(|a| {
                (
                    add_meshes(a.meshes, &mut self.textures),
                    a.joint,
                    a.offset,
                    a.weapon,
                )
            })
            .collect();
        let composition = Arc::new(Composition {
            key: bundle.key.clone(),
            rig: bundle.rig,
            parts,
            joint_nodes: bundle.joint_nodes,
            part_joints,
            plan: bundle.plan,
            facing_fix: bundle.facing_fix,
            joint_by_name: bundle.joint_by_name,
            slot_mounts: HashMap::new(),
        });
        let committed = Arc::new(Committed {
            composition,
            attachments,
            motion: bundle.motion,
            locomotion: bundle.locomotion,
        });
        self.committed.insert(bundle.key, committed.clone());
        self.stats.committed_looks += 1;
        committed
    }
}

impl Drop for ActorStreamer {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        self.shared.wake.notify_all();
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

fn worker(shared: &Shared, source: &dyn BuildSource, tx: &Sender<Built>) {
    loop {
        let job = {
            let mut jobs = shared.jobs.lock().unwrap_or_else(|p| p.into_inner());
            loop {
                if shared.stop.load(Ordering::Relaxed) {
                    return;
                }
                if !jobs.is_empty() {
                    break;
                }
                jobs = shared.wake.wait(jobs).unwrap_or_else(|p| p.into_inner());
            }
            // Nearest to the focus first.
            let focus = *shared.focus.lock().unwrap_or_else(|p| p.into_inner());
            let best = jobs
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| {
                    a.position
                        .distance_squared(focus)
                        .total_cmp(&b.position.distance_squared(focus))
                })
                .map(|(i, _)| i)
                .unwrap_or(0);
            jobs.swap_remove(best)
        };
        let start = Instant::now();
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| source.build(&job.request)))
                .unwrap_or_else(|_| Err(anyhow::anyhow!("build of {} panicked", job.key)));
        if tx.send((job.key, result, start.elapsed())).is_err() {
            return;
        }
    }
}

/// The asset stores the commit writes to.
#[derive(bevy::ecs::system::SystemParam)]
pub struct StreamAssets<'w> {
    pub meshes: ResMut<'w, Assets<Mesh>>,
    pub materials: ResMut<'w, Assets<StandardMaterial>>,
    pub images: ResMut<'w, Assets<Image>>,
    pub bindposes: ResMut<'w, Assets<SkinnedMeshInverseBindposes>>,
}

/// Spawn the entities of a committed look under `root` (as `ActorSpawner::spawn` does).
fn spawn_character(commands: &mut Commands, root: Entity, look: &Committed, stance: Stance) {
    let c = &look.composition;
    let mut animator = Animator::new(
        c.rig.clone(),
        c.plan.clone(),
        c.joint_nodes.clone(),
        c.facing_fix,
    );
    if let Some(m) = &look.motion {
        animator.play(m.clone(), 0.0, 1.0, 0.0);
    }
    animator.evaluate();
    let mut root_cmd = commands.entity(root);
    root_cmd
        .remove::<PendingActor>()
        .insert((animator, CharacterComposition(c.clone())));
    if look.locomotion {
        let mut locomotion = Locomotion::default();
        locomotion.key.stance = stance;
        root_cmd.insert((
            locomotion,
            LocomotionInput {
                stance,
                ..Default::default()
            },
        ));
    }
    let joints: Vec<Entity> = (0..c.joint_nodes.len())
        .map(|_| {
            commands
                .spawn((SkeletonJoint, GlobalTransform::default(), ChildOf(root)))
                .id()
        })
        .collect();
    let mut part_meshes = PartMeshes::default();
    for (part, part_joints) in c.parts.iter().zip(&c.part_joints) {
        let skin_joints: Vec<Entity> = part_joints.iter().map(|&j| joints[j as usize]).collect();
        let entities = part
            .meshes
            .iter()
            .map(|m| {
                commands
                    .spawn((
                        Mesh3d(m.mesh.clone()),
                        MeshMaterial3d(m.material.clone()),
                        SkinnedMesh {
                            inverse_bindposes: part.inverse_bindposes.clone(),
                            joints: skin_joints.clone(),
                        },
                        part.aabb,
                        Transform::default(),
                        Visibility::default(),
                        ChildOf(root),
                    ))
                    .id()
            })
            .collect();
        part_meshes.0.push((part.name.clone(), entities));
    }
    for (meshes, joint, offset, weapon) in &look.attachments {
        let joint = match weapon {
            Some((drawn, holster)) if !stance.weapon_drawn() => {
                let _ = drawn;
                *holster
            }
            _ => *joint,
        };
        for m in meshes {
            let mut e = commands.spawn((
                Mesh3d(m.mesh.clone()),
                MeshMaterial3d(m.material.clone()),
                Attachment {
                    joint,
                    offset: *offset,
                },
                Transform::default(),
                Visibility::default(),
                NoFrustumCulling,
                ChildOf(root),
            ));
            if let Some((drawn, holster)) = weapon {
                e.insert(WeaponMount {
                    drawn_joint: *drawn,
                    holster_joint: *holster,
                });
            }
        }
    }
    commands
        .entity(root)
        .insert((part_meshes, CharacterJoints(joints)));
}

/// Collect finished builds and commit nearest-first within the frame budget.
pub fn stream_commit_system(
    mut commands: Commands,
    streamer: Option<ResMut<ActorStreamer>>,
    mut assets: StreamAssets,
    focus: Query<&GlobalTransform, With<StreamFocus>>,
    alive: Query<(), With<PendingActor>>,
) {
    let Some(mut streamer) = streamer else {
        return;
    };
    let s = &mut *streamer;
    let frame_start = Instant::now();
    s.stats.spawned_this_frame = 0;
    let focus = focus.iter().next().map_or(Vec3::ZERO, |t| t.translation());
    s.set_focus(focus);
    s.collect();
    // Drop cancelled requests and those whose look failed.
    let failed = &s.failed;
    s.pending
        .retain(|p| alive.contains(p.entity) && !failed.contains_key(&p.key));
    let mut ready_now: Vec<usize> = (0..s.pending.len())
        .filter(|&i| {
            let k = &s.pending[i].key;
            s.committed.contains_key(k) || s.ready.contains_key(k)
        })
        .collect();
    ready_now.sort_by(|&a, &b| {
        s.pending[a]
            .position
            .distance_squared(focus)
            .total_cmp(&s.pending[b].position.distance_squared(focus))
    });
    let mut done = Vec::new();
    for i in ready_now {
        if s.stats.spawned_this_frame >= s.config.max_per_frame
            || (s.stats.spawned_this_frame > 0 && frame_start.elapsed() >= s.config.frame_budget)
        {
            break;
        }
        let one = Instant::now();
        let key = s.pending[i].key.clone();
        let look = match s.committed.get(&key) {
            Some(c) => c.clone(),
            None => {
                let bundle = s.ready.remove(&key).expect("ready bundle");
                s.commit_assets(bundle, &mut assets)
            }
        };
        let p = &s.pending[i];
        spawn_character(&mut commands, p.entity, &look, p.stance);
        s.stats.spawned += 1;
        s.stats.spawned_this_frame += 1;
        s.stats.max_commit_one = s.stats.max_commit_one.max(one.elapsed());
        done.push(i);
    }
    done.sort_unstable();
    for i in done.into_iter().rev() {
        s.pending.swap_remove(i);
    }
    s.stats.commit_this_frame = frame_start.elapsed();
    s.stats.max_commit_frame = s.stats.max_commit_frame.max(s.stats.commit_this_frame);
    s.stats.queued = s.shared.jobs.lock().map_or(0, |j| j.len());
    s.stats.building = s.in_flight.len().saturating_sub(s.stats.queued);
    s.stats.pending_spawns = s.pending.len();
}

/// Adds [`stream_commit_system`] (Update). Insert an [`ActorStreamer`] to use it.
pub struct ActorStreamPlugin;

impl bevy::app::Plugin for ActorStreamPlugin {
    fn build(&self, app: &mut bevy::app::App) {
        app.add_systems(bevy::app::Update, stream_commit_system);
    }
}
