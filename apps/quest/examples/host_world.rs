//! Host verification of the streamed world + NPCs (needs the local world pack `.local/quest/world-pack`, see
//! `examples/pack_world.rs`, and a GPU): teleports to a few towns, waits for the streamer and the NPC spawner, renders
//! `.local/quest/host_world_<town>_{street,over,npc}.png`, then walks a path through the world at 72 Hz simulated time
//! and prints per-frame CPU of the streaming and NPC systems.
//!
//! ```text
//! host_world [--only ardea|vengard|montera|faring] [--no-shots] [--bench-frames N] [--no-bench]
//! ```
use std::time::{Duration, Instant};

use bevy::asset::AssetPlugin;
use bevy::camera::RenderTarget;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::time::TimeUpdateStrategy;
use gothic_quest::ardea;
use gothic_quest::npcs::Npcs;
use gothic_quest::qmat::QuestMaterialPlugin;
use gothic_quest::world::WorldStream;
use gothic_vr::VrPlayer;

#[derive(Component)]
struct ShotCam;

/// Gothic-space (x, z) of towns: the median of the original NPC placements of the town's layers.
const TOWNS: [(&str, f32, f32); 4] = [
    ("ardea", 92_400.0, -11_000.0),
    ("vengard", 94_396.0, 33_922.0),
    ("montera", 13_351.0, 21_500.0),
    ("faring", 61_241.0, 57_110.0),
];

fn teleport(app: &mut App, gx: f32, gz: f32) {
    let w = app.world_mut();
    let mut p = w.resource_mut::<VrPlayer>();
    p.ctrl.teleport(Vec3::new(-gx, 20_000.0, gz));
    w.resource_mut::<gothic_quest::locomotion::Physics>().ready = false;
}

/// Updates (sleeping a little so the workers run) until the streamer is idle and the NPC set is stable.
fn settle(app: &mut App, max_frames: usize) -> usize {
    let mut stable = 0;
    let mut last_live = usize::MAX;
    for f in 0..max_frames {
        app.update();
        std::thread::sleep(Duration::from_millis(6));
        let ready = app
            .world()
            .resource::<gothic_quest::locomotion::Physics>()
            .ready
            && app.world().resource::<WorldStream>().all_ready();
        let (live, jobs) = app
            .world()
            .get_resource::<Npcs>()
            .map_or((0, 0), |n| (n.stats.live, 0usize));
        let _ = jobs;
        if ready && live == last_live {
            stable += 1;
        } else {
            stable = 0;
        }
        last_live = live;
        if stable > 90 {
            return f;
        }
    }
    max_frames
}

fn pcts(v: &mut [f32]) -> (f32, f32, f32, f32) {
    if v.is_empty() {
        return (0.0, 0.0, 0.0, 0.0);
    }
    v.sort_by(|a, b| a.total_cmp(b));
    let avg = v.iter().sum::<f32>() / v.len() as f32;
    let p = |q: f32| v[((v.len() as f32 * q) as usize).min(v.len() - 1)];
    (avg, p(0.95), p(0.99), *v.last().unwrap())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let flag = |f: &str| args.iter().any(|a| a == f);
    let val = |f: &str| {
        args.iter()
            .position(|a| a == f)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let only = val("--only");
    let bench_frames: usize = val("--bench-frames")
        .and_then(|v| v.parse().ok())
        .unwrap_or(4_000);
    let dir = ardea::data_dir().expect("run from the repo root with .local/quest/world-pack present");
    assert!(dir.join("world.idx").is_file(), "{} is not a world pack", dir.display());
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                ..default()
            })
            .set(AssetPlugin::default())
            .disable::<bevy::log::LogPlugin>(),
    );
    app.add_plugins(QuestMaterialPlugin);
    app.insert_resource(gothic_xr_quest::XrFrameData::default());
    // Simulated 72 Hz time so the animation update rates behave like on the headset.
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / 72.0,
    )));
    ardea::build(&mut app, dir.clone()).expect("world build");
    gothic_quest::npcs::install(&mut app, &dir);
    app.insert_resource(ClearColor(Color::srgb(0.55, 0.64, 0.76)));
    app.finish();
    app.cleanup();
    let out = std::path::PathBuf::from(".local/quest");
    let size = Extent3d {
        width: 1400,
        height: 900,
        depth_or_array_layers: 1,
    };
    let mut img = Image::new_fill(
        size,
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::default(),
    );
    img.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING
        | TextureUsages::COPY_DST
        | TextureUsages::COPY_SRC
        | TextureUsages::RENDER_ATTACHMENT;
    let handle = app.world_mut().resource_mut::<Assets<Image>>().add(img);
    app.world_mut().spawn((
        Camera3d::default(),
        Camera {
            clear_color: ClearColorConfig::Custom(Color::srgb(0.55, 0.64, 0.76)),
            ..default()
        },
        RenderTarget::Image(handle.clone().into()),
        Projection::Perspective(PerspectiveProjection {
            fov: 1.6,
            near: 0.05,
            ..default()
        }),
        RenderLayers::layer(0),
        gothic_xr_quest::XrEye(0),
        ShotCam,
        Transform::IDENTITY,
    ));
    app.world_mut().spawn((
        DirectionalLight {
            illuminance: 9000.0,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.9, 0.5, 0.0)),
    ));
    app.world_mut().insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.75, 0.8, 1.0),
        brightness: 500.0,
        ..default()
    });
    for _ in 0..3 {
        app.update();
    }

    if flag("--walk-check") {
        // Device repro: the hero controller at the Ardea start spot, full stick for 3 s at 72 Hz against the real
        // streamed collision. Prints distance, grounded, mode and speed every 0.25 s.
        let (sx, sz) = (92_880.0, -11_997.0);
        teleport(&mut app, sx, sz);
        settle(&mut app, 3_000);
        let jt: f32 = std::env::var("JUMP_TIME").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0);
        app.world_mut().resource_scope(|w, vw: Mut<gothic_vr::VrWorld>| {
        let world = &vw.0;
        let mut p = w.resource_mut::<VrPlayer>();
        p.ctrl.config.gravity = 981.0 / (jt * jt);
        p.ctrl.config.jump_velocity = 600.0 / jt;
        let start = p.ctrl.position;
        println!("[walk] start {:?} grounded {} mode {:?} gravity {}", start, p.ctrl.grounded, p.ctrl.mode, p.ctrl.config.gravity);
        for i in 0..216 {
            let wish = Vec3::new(0.0, 0.0, 600.0 * if i % 2 == 0 { 0.97 } else { 0.96 });
            p.ctrl.step(world, wish, false, 1.0 / 72.0);
            if i % 18 == 17 {
                let d = p.ctrl.position - start;
                println!("[walk] t={:.2}s dist_xz={:.0} cm dy={:.0} grounded={} mode={:?} vel={:.0}", (i + 1) as f32 / 72.0, Vec2::new(d.x, d.z).length(), d.y, p.ctrl.grounded, p.ctrl.mode, p.ctrl.velocity.length());
            }
        }
        });
        return;
    }
    if !flag("--no-shots") {
        for (name, gx, gz) in TOWNS {
            if only.as_deref().is_some_and(|o| o != name) {
                continue;
            }
            let t0 = Instant::now();
            teleport(&mut app, gx, gz);
            let frames = settle(&mut app, 3_000);
            let (pp, live, cells, ents, mem, tris) = {
                let w = app.world();
                let p = w.resource::<VrPlayer>().ctrl.position;
                let ws = w.resource::<WorldStream>();
                (
                    p,
                    w.get_resource::<Npcs>().map_or(0, |n| n.stats.live),
                    ws.stats.cells,
                    ws.stats.entities,
                    ws.stats.mem_mb,
                    ws.stats.tris,
                )
            };
            println!(
                "[{name}] settled after {frames} frames / {:.1}s: ground y {:.0} cm, {cells} cells, {ents} statics, {tris} tris, {mem:.0} MB, {live} NPCs live",
                t0.elapsed().as_secs_f32(),
                pp.y
            );
            // Live NPCs: the one with the most neighbours within 8 m is the subject.
            let live: Vec<Vec3> = app
                .world()
                .get_resource::<Npcs>()
                .map(|n| n.live_positions())
                .unwrap_or_default();
            let subject = live
                .iter()
                .copied()
                .max_by_key(|p| live.iter().filter(|q| q.xz().distance(p.xz()) < 800.0).count())
                .unwrap_or(pp + Vec3::new(0.0, 0.0, 2_000.0));
            let group: Vec<Vec3> = live
                .iter()
                .copied()
                .filter(|q| q.xz().distance(subject.xz()) < 800.0)
                .collect();
            let centroid = if group.is_empty() {
                subject
            } else {
                group.iter().copied().sum::<Vec3>() / group.len() as f32
            };
            let gy = pp.y;
            let m = |v: Vec3| v * 0.01;
            let away = Vec3::new(pp.x - centroid.x, 0.0, pp.z - centroid.z).normalize_or_zero();
            let away = if away == Vec3::ZERO { Vec3::Z } else { away };
            let eye = m(Vec3::new(centroid.x, gy + 170.0, centroid.z) + away * 900.0);
            let street_target = m(Vec3::new(centroid.x, gy + 110.0, centroid.z));
            let npc_eye = m(Vec3::new(subject.x, gy + 150.0, subject.z) + away * 330.0);
            let npc_target = m(Vec3::new(subject.x, gy + 105.0, subject.z));
            let over_eye = m(Vec3::new(centroid.x, gy + 2_600.0, centroid.z) + away * 1_500.0);
            let shots = [
                ("street", eye, street_target),
                ("over", over_eye, street_target),
                ("npc", npc_eye, npc_target),
            ];
            for (tag, from, to) in shots {
                {
                    let mut q = app
                        .world_mut()
                        .query_filtered::<&mut Transform, With<ShotCam>>();
                    for mut t in q.iter_mut(app.world_mut()) {
                        *t = Transform::from_translation(from).looking_at(to, Vec3::Y);
                    }
                }
                // Let the NPC systems react to the new view (cone culling, LOD swaps, uploads).
                for _ in 0..90 {
                    app.update();
                    std::thread::sleep(Duration::from_millis(4));
                }
                let path = out.join(format!("host_world_{name}_{tag}.png"));
                app.world_mut()
                    .spawn(Screenshot::image(handle.clone()))
                    .observe(save_to_disk(path.clone()));
                for _ in 0..10 {
                    app.update();
                }
                println!("wrote {}", path.display());
            }
        }
    }

    if !flag("--no-bench") {
        if flag("--no-render") {
            // Main-world CPU only: the offscreen camera is switched off.
            let mut q = app.world_mut().query_filtered::<&mut Camera, With<ShotCam>>();
            for mut c in q.iter_mut(app.world_mut()) {
                c.is_active = false;
            }
        }
        bench(&mut app, bench_frames);
    }
}

fn bench(app: &mut App, frames: usize) {
    // Walk from Ardea toward Vengard (about 450 m) at 6 m/s, then loop back.
    let (a, b) = (TOWNS[0], TOWNS[1]);
    let from = Vec3::new(-a.1, 0.0, a.2);
    let to = Vec3::new(-b.1, 0.0, b.2);
    teleport(app, a.1, a.2);
    settle(app, 3_000);
    let y = app.world().resource::<VrPlayer>().ctrl.position.y;
    let len = from.distance(to);
    let speed_cm = 600.0 / 72.0;
    let mut stream = Vec::new();
    let mut manage = Vec::new();
    let mut anim = Vec::new();
    let mut wall = Vec::new();
    let mut live = Vec::new();
    let mut vis = Vec::new();
    let mut animated = Vec::new();
    let mut ents = Vec::new();
    let mut peak_mem = 0.0f32;
    let mut vis_meshes = Vec::new();
    let mut vis_tris = Vec::new();
    let mut s = 0.0f32;
    // The streamer's own peak counters restart for the measured run.
    {
        let mut ws = app.world_mut().resource_mut::<WorldStream>();
        ws.stats.us_peak = 0;
        ws.stats.collision_us_peak = 0;
    }
    for f in 0..frames {
        s += speed_cm;
        let u = (s / len) % 2.0;
        let u = if u > 1.0 { 2.0 - u } else { u };
        let p = from.lerp(to, u);
        {
            let mut pl = app.world_mut().resource_mut::<VrPlayer>();
            pl.ctrl.position = Vec3::new(p.x, y, p.z);
        }
        // Face along the walking direction.
        {
            let dir = (to - from).normalize() * if (s / len) % 2.0 > 1.0 { -1.0 } else { 1.0 };
            let eye = Vec3::new(p.x, y + 170.0, p.z) * 0.01;
            let mut q = app
                .world_mut()
                .query_filtered::<&mut Transform, With<ShotCam>>();
            for mut t in q.iter_mut(app.world_mut()) {
                *t = Transform::from_translation(eye)
                    .looking_at(eye + Vec3::new(dir.x, -0.05, dir.z), Vec3::Y);
            }
        }
        let t0 = Instant::now();
        app.update();
        wall.push(t0.elapsed().as_secs_f32() * 1e3);
        if f < 120 {
            continue; // warm-up
        }
        if f % 24 == 0 {
            let (mut n, mut t) = (0f32, 0f32);
            let mut q = app
                .world_mut()
                .query::<(&ViewVisibility, &gothic_quest::ardea::TriCount)>();
            for (v, tc) in q.iter(app.world()) {
                if v.get() {
                    n += 1.0;
                    t += tc.0 as f32;
                }
            }
            vis_meshes.push(n);
            vis_tris.push(t);
        }
        let w = app.world();
        let ws = w.resource::<WorldStream>();
        stream.push(ws.stats.us_last as f32 / 1e3);
        ents.push(ws.stats.entities as f32);
        peak_mem = peak_mem.max(ws.stats.mem_mb);
        if let Some(n) = w.get_resource::<Npcs>() {
            manage.push(n.stats.manage_us_last as f32 / 1e3);
            anim.push(n.stats.anim_us_last as f32 / 1e3);
            live.push(n.stats.live as f32);
            vis.push(n.stats.visible as f32);
            animated.push(n.stats.animated_last as f32);
        }
    }
    let ws = app.world().resource::<WorldStream>();
    let line = |name: &str, v: &mut Vec<f32>| {
        let (avg, p95, p99, max) = pcts(v);
        println!("{name:<28} avg {avg:7.3}  p95 {p95:7.3}  p99 {p99:7.3}  max {max:7.3}");
    };
    println!("--- CPU ms per frame over {} frames at simulated 72 Hz, walking 6 m/s ---", stream.len());
    line("stream_system (main thread)", &mut stream);
    line("npc_manage (spawn/despawn/LOD)", &mut manage);
    line("npc_animate (cull + anim)", &mut anim);
    let mut both: Vec<f32> = stream
        .iter()
        .zip(manage.iter().zip(&anim))
        .map(|(a, (b, c))| a + b + c)
        .collect();
    line("stream + npc total", &mut both);
    line("app.update wall (host GPU)", &mut wall);
    let (la, _, _, lm) = pcts(&mut live);
    let (va, _, _, vm) = pcts(&mut vis);
    let (aa, _, _, am) = pcts(&mut animated);
    let (ea, _, _, em) = pcts(&mut ents);
    println!(
        "NPCs live avg {la:.1} max {lm:.0} | visible avg {va:.1} max {vm:.0} | animated per frame avg {aa:.1} max {am:.0} | static entities avg {ea:.0} max {em:.0} | resident asset mem peak {peak_mem:.0} MB"
    );
    let (vma, _, _, vmm) = pcts(&mut vis_meshes);
    let (vta, _, _, vtm) = pcts(&mut vis_tris);
    println!(
        "visible statics (camera view, 100 deg-ish): meshes avg {vma:.0} max {vmm:.0} | triangles avg {:.0}k max {:.0}k (per eye; only meaningful with rendering on)",
        vta / 1e3,
        vtm / 1e3
    );
    println!(
        "world: loaded {} cells, unloaded {}, spawned {} statics, collision insert peak {} us",
        ws.stats.loaded_total,
        ws.stats.unloaded_total,
        ws.stats.spawned_total,
        ws.stats.collision_us_peak
    );
}
