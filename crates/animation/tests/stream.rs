//! The streamer in a headless app with the synthetic source: everything requested gets
//! spawned with the same components as `ActorSpawner::spawn`, looks are built once, the
//! budget, priorities, cancellation and failures behave.
use bevy::asset::AssetPlugin;
use bevy::ecs::system::RunSystemOnce;
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use gothic_animation::runtime::{CharacterJoints, PartMeshes, SkeletonJoint};
use gothic_animation::stream::{AttachRequest, StreamAssets};
use gothic_animation::stream_synthetic::SyntheticSource;
use gothic_animation::{
    ActorStreamPlugin, ActorStreamer, Animator, BuildRequest, PendingActor, Stance, StreamConfig,
    StreamFocus,
};
use std::sync::Arc;
use std::time::Duration;

fn small_source() -> SyntheticSource {
    SyntheticSource {
        joints: 16,
        vertices_per_part: 400,
        texture_size: 32,
        fail_marker: Some("Broken".into()),
    }
}

fn app(config: StreamConfig) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Mesh>()
        .init_asset::<Image>()
        .init_asset::<StandardMaterial>()
        .init_asset::<SkinnedMeshInverseBindposes>()
        .add_plugins(ActorStreamPlugin)
        .insert_resource(ActorStreamer::new(Arc::new(small_source()), config));
    app
}

fn look(i: usize) -> BuildRequest {
    BuildRequest::new([format!("Body_{}", i % 2), format!("Head_{i}")])
}

fn request(app: &mut App, req: BuildRequest, at: Vec3) -> Entity {
    let world = app.world_mut();
    world.resource_scope(|world, mut s: Mut<ActorStreamer>| {
        let mut commands = world.commands();
        let e = s.request(
            &mut commands,
            req,
            Transform::from_translation(at),
            Stance::default(),
        );
        world.flush();
        e
    })
}

fn run_until_idle(app: &mut App, max_frames: usize) -> usize {
    for f in 0..max_frames {
        app.update();
        if app.world().resource::<ActorStreamer>().is_idle() {
            return f + 1;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    panic!("streamer did not finish in {max_frames} frames");
}

#[test]
fn everything_requested_is_spawned_and_looks_build_once() {
    let mut app = app(StreamConfig::default());
    let roots: Vec<Entity> = (0..40)
        .map(|i| request(&mut app, look(i % 8), Vec3::new(i as f32 * 100.0, 0.0, 0.0)))
        .collect();
    // Placeholders exist immediately.
    assert_eq!(
        app.world_mut()
            .query::<&PendingActor>()
            .iter(app.world())
            .count(),
        40
    );
    run_until_idle(&mut app, 2000);
    let world = app.world_mut();
    for &r in &roots {
        assert!(world.get::<PendingActor>(r).is_none());
        assert!(world.get::<Animator>(r).is_some());
        let joints = world.get::<CharacterJoints>(r).unwrap().0.clone();
        assert_eq!(joints.len(), 16);
        assert!(world.get::<SkeletonJoint>(joints[0]).is_some());
        let parts = world.get::<PartMeshes>(r).unwrap();
        assert_eq!(parts.0.len(), 2);
    }
    let s = world.resource::<ActorStreamer>();
    assert_eq!(s.stats.built, 8, "one build per look");
    assert_eq!(s.stats.committed_looks, 8);
    assert_eq!(s.stats.spawned, 40);
    // Two body parts and 8 heads share textures by part name: 10 images.
    assert_eq!(world.resource::<Assets<Image>>().len(), 10);
}

#[test]
fn budget_limits_commits_per_frame() {
    let mut app = app(StreamConfig {
        workers: 2,
        frame_budget: Duration::ZERO,
        max_per_frame: 8,
    });
    for i in 0..12 {
        request(&mut app, look(0), Vec3::new(i as f32, 0.0, 0.0));
    }
    let mut max_per_frame = 0;
    for _ in 0..2000 {
        app.update();
        let s = app.world().resource::<ActorStreamer>();
        max_per_frame = max_per_frame.max(s.stats.spawned_this_frame);
        if s.is_idle() {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(app.world().resource::<ActorStreamer>().stats.spawned, 12);
    assert_eq!(
        max_per_frame, 1,
        "a zero budget still commits one per frame"
    );
}

#[test]
fn nearest_to_the_focus_spawns_first() {
    let mut app = app(StreamConfig {
        workers: 1,
        frame_budget: Duration::ZERO,
        max_per_frame: 1,
    });
    app.world_mut().spawn((
        StreamFocus,
        GlobalTransform::from_translation(Vec3::new(1000.0, 0.0, 0.0)),
    ));
    // One look, so all are ready together after the single build.
    let far = request(&mut app, look(3), Vec3::ZERO);
    let near = request(&mut app, look(3), Vec3::new(990.0, 0.0, 0.0));
    let mid = request(&mut app, look(3), Vec3::new(500.0, 0.0, 0.0));
    let mut order = Vec::new();
    for _ in 0..2000 {
        app.update();
        for e in [far, near, mid] {
            if !order.contains(&e) && app.world().get::<PendingActor>(e).is_none() {
                order.push(e);
            }
        }
        if order.len() == 3 {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(order, [near, mid, far]);
}

#[test]
fn cancelled_and_failed_requests_are_dropped() {
    let mut app = app(StreamConfig::default());
    let cancelled = request(&mut app, look(5), Vec3::ZERO);
    let broken = request(&mut app, BuildRequest::new(["Broken_Body"]), Vec3::ZERO);
    let ok = request(&mut app, look(6), Vec3::ZERO);
    app.world_mut().despawn(cancelled);
    run_until_idle(&mut app, 2000);
    let world = app.world();
    assert!(world.get::<Animator>(ok).is_some());
    assert!(world.get::<Animator>(broken).is_none());
    let s = world.resource::<ActorStreamer>();
    assert!(
        s.failure("Broken_Body")
            .unwrap()
            .contains("synthetic failure")
    );
    assert_eq!(s.stats.spawned, 1);
}

#[test]
fn attachments_and_blocking_spawn() {
    let mut app = app(StreamConfig::default());
    let req = BuildRequest {
        attachments: vec![AttachRequest {
            mesh: "Sword".into(),
            slot: "Slot_RightHand".into(),
            holster_slot: Some("Slot_Back".into()),
        }],
        ..look(1)
    };
    let world = app.world_mut();
    let e = world
        .run_system_once(
            move |mut commands: Commands,
                  mut s: ResMut<ActorStreamer>,
                  mut assets: StreamAssets| {
                s.spawn_blocking(
                    &mut commands,
                    &mut assets,
                    req.clone(),
                    Transform::default(),
                    Stance::default(),
                )
                .unwrap()
            },
        )
        .unwrap();
    let world = app.world_mut();
    let children: Vec<Entity> = world
        .get::<Children>(e)
        .map(|c| c.iter().collect())
        .unwrap_or_default();
    let mounts = children
        .iter()
        .filter(|c| {
            world
                .get::<gothic_animation::runtime::WeaponMount>(**c)
                .is_some()
        })
        .count();
    assert_eq!(mounts, 1);
    assert!(world.get::<PendingActor>(e).is_none());
}
