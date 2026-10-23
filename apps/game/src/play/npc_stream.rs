//! Opt-in streamed NPC bodies: `--stream-actors` or `GR_STREAM_ACTORS=1`.
//!
//! Without the flag nothing here is installed and `npcs::spawn_npcs` builds every body on the
//! main thread with `ActorSpawner::spawn`, as before. With it, queued characters are requested
//! from a [`gothic_animation::ActorStreamer`] over a [`LibrarySource`] (the same loading code,
//! on worker threads); the walk camera is the [`StreamFocus`], so the nearest characters are
//! built and committed first. A body gets its presentation components and its core actor
//! (`npcs::bind_spawned`) once committed. A look that fails with weapons is requested again
//! without them, like the blocking path's retry. The hero is always spawned blocking.
//! See `docs/ACTOR_STREAMING.md`.
use super::Roster;
use super::npcs::{Placement, bind_spawned};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use gothic_animation::stream::stream_commit_system;
use gothic_animation::{
    ActorStreamPlugin, ActorStreamer, BuildRequest, LibrarySource, NpcSpec, PendingActor, Stance,
    StreamConfig, StreamFocus,
};
use gothic_runtime::CoreCommand;
use std::path::Path;
use std::sync::{Arc, OnceLock};

/// `--stream-actors` on the command line or `GR_STREAM_ACTORS` set to anything but `0`/empty.
pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        std::env::args().any(|a| a == "--stream-actors")
            || std::env::var("GR_STREAM_ACTORS").is_ok_and(|v| !v.is_empty() && v != "0")
    })
}

/// Installs the streamer when [`enabled`]; otherwise does nothing.
pub fn register(app: &mut App, data_dir: &Path) {
    if !enabled() {
        return;
    }
    let config = StreamConfig::default();
    eprintln!(
        "play: NPC bodies are streamed ({} build workers)",
        config.workers
    );
    app.insert_resource(ActorStreamer::new(
        Arc::new(LibrarySource::open(data_dir)),
        config,
    ))
    .add_plugins(ActorStreamPlugin)
    .add_systems(
        Update,
        (focus_camera, bind_streamed.after(stream_commit_system)),
    )
    .add_systems(Last, report_stats);
}

/// Requested NPC body waiting for its commit.
#[derive(Component)]
struct StreamedNpc {
    key: String,
    /// The same look without weapons, tried when the armed one fails.
    bare: Option<BuildRequest>,
    stance: Stance,
    placement: Option<Placement>,
}

/// Bodies requested from the streamer and not committed (or failed) yet: `visual` is set
/// while the candidate is not in `live`. Always 0 without `--stream-actors`.
pub fn in_flight(roster: &Roster) -> usize {
    if !enabled() {
        return 0;
    }
    let requested = roster.cands.iter().filter(|c| c.visual.is_some()).count();
    requested.saturating_sub(roster.live.len())
}

/// The streaming side of `npcs::spawn_npcs`.
#[derive(SystemParam)]
pub struct NpcStream<'w> {
    streamer: Option<ResMut<'w, ActorStreamer>>,
}

impl NpcStream<'_> {
    pub fn active(&self) -> bool {
        self.streamer.is_some()
    }

    /// Requests the body of `spec` at `tf`; it is bound to the roster when committed.
    pub fn request(
        &mut self,
        commands: &mut Commands,
        roster: &mut Roster,
        spec: &NpcSpec,
        tf: Transform,
        placement: Placement,
    ) {
        let Some(streamer) = self.streamer.as_deref_mut() else {
            return;
        };
        let ci = placement.ci;
        let request = match BuildRequest::from_spec(spec) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("play: spawn {} failed: {e:#}", roster.cands[ci].record.name);
                roster.cands[ci].failed = true;
                return;
            }
        };
        let bare = (!spec.weapons.is_empty())
            .then(|| {
                let mut bare = spec.clone();
                bare.weapons.clear();
                BuildRequest::from_spec(&bare).ok()
            })
            .flatten();
        let key = request.key();
        let entity = streamer.request(commands, request, tf, spec.stance);
        commands.entity(entity).insert(StreamedNpc {
            key,
            bare,
            stance: spec.stance,
            placement: Some(placement),
        });
        roster.cands[ci].visual = Some(entity);
    }
}

/// The walk camera decides which characters are built and committed first.
fn focus_camera(
    mut commands: Commands,
    cameras: Query<Entity, (With<Camera3d>, Without<StreamFocus>)>,
    focused: Query<(), With<StreamFocus>>,
) {
    if !focused.is_empty() {
        return;
    }
    if let Some(cam) = cameras.iter().next() {
        commands.entity(cam).insert(StreamFocus);
    }
}

/// Binds committed bodies to the roster and the core; retries failed armed looks bare.
fn bind_streamed(
    mut roster: ResMut<Roster>,
    mut streamer: ResMut<ActorStreamer>,
    mut commands: Commands,
    mut core_commands: MessageWriter<CoreCommand>,
    mut q: Query<(Entity, &mut StreamedNpc, Has<PendingActor>, &Transform)>,
) {
    for (entity, mut s, pending, tf) in &mut q {
        if !pending {
            commands.entity(entity).remove::<StreamedNpc>();
            if let Some(placement) = s.placement.take() {
                bind_spawned(
                    &mut roster,
                    &mut commands,
                    &mut core_commands,
                    entity,
                    placement,
                );
            }
            continue;
        }
        let Some(why) = streamer.failure(&s.key).map(str::to_string) else {
            continue;
        };
        let Some(ci) = s.placement.as_ref().map(|p| p.ci) else {
            continue;
        };
        let name = roster.cands[ci].record.name.clone();
        match s.bare.take() {
            Some(bare) => {
                eprintln!("play: {name} spawn with weapons failed ({why}), retrying bare");
                s.key = bare.key();
                let stance = s.stance;
                streamer.request_on(&mut commands, entity, bare, tf.translation, stance);
            }
            None => {
                eprintln!("play: spawn {name} failed: {why}");
                let cand = &mut roster.cands[ci];
                cand.failed = true;
                cand.visual = None;
                commands.entity(entity).despawn();
            }
        }
    }
}

/// One line per burst once the streamer runs dry: what the benchmark compares.
fn report_stats(streamer: Res<ActorStreamer>, mut was_busy: Local<bool>) {
    let idle = streamer.is_idle();
    if *was_busy && idle {
        let s = &streamer.stats;
        eprintln!(
            "play: actor stream idle: {} spawned, {} looks built ({} failed), build total {:.0} ms / max {:.1} ms, commit max {:.2} ms per frame / {:.2} ms per character",
            s.spawned,
            s.built,
            s.failed,
            s.build_total.as_secs_f64() * 1e3,
            s.max_build.as_secs_f64() * 1e3,
            s.max_commit_frame.as_secs_f64() * 1e3,
            s.max_commit_one.as_secs_f64() * 1e3,
        );
    }
    *was_busy = !idle;
}
