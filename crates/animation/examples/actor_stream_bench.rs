//! Spawn benchmark: 200 characters (4 bodies x `--looks` heads x `--looks` armours, about 120
//! distinct looks) walk into view in bursts while the app runs; frame times are measured and
//! written as JSON. `--mode stream` builds on worker threads and commits within a frame budget;
//! `--mode blocking` builds on the main thread when requested (the behaviour before streaming).
//!
//! `cargo run --release -p gothic-animation --example actor_stream_bench -- \
//!     [--mode stream|blocking] [--actors 200] [--looks 40] [--out bench.json]`
use bevy::asset::AssetPlugin;
use bevy::ecs::system::RunSystemOnce;
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use gothic_animation::stream::StreamAssets;
use gothic_animation::stream_synthetic::SyntheticSource;
use gothic_animation::{
    ActorStreamPlugin, ActorStreamer, AnimationPlugin, BuildRequest, Stance, StreamConfig,
    StreamFocus,
};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn arg(name: &str) -> Option<String> {
    let a: Vec<String> = std::env::args().collect();
    a.iter()
        .position(|x| x == name)
        .and_then(|i| a.get(i + 1))
        .cloned()
}

fn main() {
    let mode = arg("--mode").unwrap_or_else(|| "stream".into());
    let actors: usize = arg("--actors").and_then(|v| v.parse().ok()).unwrap_or(200);
    let looks: usize = arg("--looks").and_then(|v| v.parse().ok()).unwrap_or(40);
    let out = arg("--out").unwrap_or_else(|| "actor_stream_bench.json".into());
    let blocking = mode == "blocking";

    let source = Arc::new(SyntheticSource::default());
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), AnimationPlugin))
        .init_asset::<Mesh>()
        .init_asset::<Image>()
        .init_asset::<StandardMaterial>()
        .init_asset::<SkinnedMeshInverseBindposes>()
        .add_plugins(ActorStreamPlugin)
        .insert_resource(ActorStreamer::new(source, StreamConfig::default()));
    app.world_mut()
        .spawn((StreamFocus, GlobalTransform::default()));
    app.finish();
    app.cleanup();

    // Warm-up frames, then bursts of 20 requests every 10 frames.
    let warmup = 30;
    let burst = 20;
    let total_frames = warmup + (actors / burst + 1) * 10 + 400;
    let mut frame_ms = Vec::with_capacity(total_frames);
    let mut requested = 0usize;
    let mut keys = std::collections::HashSet::new();
    let mut all_spawned_at = None;
    for frame in 0..total_frames {
        if frame >= warmup && (frame - warmup) % 10 == 0 && requested < actors {
            let n = burst.min(actors - requested);
            let base = requested;
            let reqs: Vec<(BuildRequest, Transform)> = (0..n)
                .map(|k| {
                    let i = base + k;
                    let req = BuildRequest::new([
                        format!("Body_{}", i % 4),
                        format!("Head_{}", i % looks),
                        format!("Armor_{}", (i / 3) % looks),
                    ]);
                    let at = Vec3::new((i % 20) as f32 * 300.0, 0.0, (i / 20) as f32 * 300.0);
                    (req, Transform::from_translation(at))
                })
                .collect();
            requested += n;
            keys.extend(reqs.iter().map(|(r, _)| r.key()));
            let t = Instant::now();
            app.world_mut()
                .run_system_once(
                    move |mut commands: Commands,
                          mut s: ResMut<ActorStreamer>,
                          mut assets: StreamAssets| {
                        for (req, tr) in reqs.clone() {
                            if blocking {
                                s.spawn_blocking(
                                    &mut commands,
                                    &mut assets,
                                    req,
                                    tr,
                                    Stance::default(),
                                )
                                .expect("build");
                            } else {
                                s.request(&mut commands, req, tr, Stance::default());
                            }
                        }
                    },
                )
                .expect("request system");
            app.update();
            frame_ms.push(t.elapsed().as_secs_f64() * 1000.0);
        } else {
            let t = Instant::now();
            app.update();
            frame_ms.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        let s = app.world().resource::<ActorStreamer>();
        if all_spawned_at.is_none() && s.stats.spawned >= actors {
            all_spawned_at = Some(frame);
        }
        // Pace like a 60 Hz game so worker threads get wall time between frames.
        std::thread::sleep(Duration::from_millis(4));
    }
    let s = &app.world().resource::<ActorStreamer>().stats;
    let measured: Vec<f64> = frame_ms[warmup..].to_vec();
    let mut sorted = measured.clone();
    sorted.sort_by(f64::total_cmp);
    let pct = |p: f64| sorted[((sorted.len() - 1) as f64 * p).round() as usize];
    let over = |ms: f64| measured.iter().filter(|&&x| x > ms).count();
    let distinct = keys.len();
    let report = serde_json::json!({
        "mode": mode,
        "actors": actors,
        "looks_param": looks,
        "distinct_looks": distinct,
        "looks_built": s.built,
        "spawned": s.spawned,
        "frames": measured.len(),
        "frame_ms": {
            "mean": measured.iter().sum::<f64>() / measured.len() as f64,
            "p50": pct(0.5),
            "p99": pct(0.99),
            "max": sorted.last().copied().unwrap_or(0.0),
        },
        "frames_over_4ms": over(4.0),
        "frames_over_16ms": over(16.0),
        "max_commit_frame_ms": s.max_commit_frame.as_secs_f64() * 1000.0,
        "max_commit_one_ms": s.max_commit_one.as_secs_f64() * 1000.0,
        "build_ms": {
            "total": s.build_total.as_secs_f64() * 1000.0,
            "max": s.max_build.as_secs_f64() * 1000.0,
            "mean": s.build_total.as_secs_f64() * 1000.0 / s.built.max(1) as f64,
        },
        "frames_until_all_spawned": all_spawned_at.map(|f| f - warmup),
    });
    let text = serde_json::to_string_pretty(&report).expect("json");
    println!("{text}");
    std::fs::write(&out, text).expect("write report");
}
