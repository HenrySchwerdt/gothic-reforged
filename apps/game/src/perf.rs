//! Performance tooling: F3 overlay, `--bench <seconds>` JSON frame statistics and (with the
//! `trace` Cargo feature) a span profiler that aggregates every Bevy system / schedule span.
//!
//! * `F3` toggles the overlay (fps, frame ms avg / p95 / max, main-thread CPU ms, wait ms,
//!   GPU ms when render diagnostics run, entity and visible-mesh counts).
//! * `F4` writes the span table to `.local/bench/trace-<unix>.txt` (feature `trace`).
//! * `--bench <seconds> [--bench-warmup s] [--bench-name n] [--bench-walk] [--bench-gpu]`
//!   runs without vsync, discards the warm-up, then writes
//!   `.local/bench/<name>-<unix>.json` and exits. `--trace` (feature `trace`) adds the span table.
use bevy::{
    app::AppExit,
    camera::visibility::ViewVisibility,
    diagnostic::{DiagnosticsStore, EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin},
    prelude::*,
    window::{PresentMode, PrimaryWindow},
};
use std::{
    collections::VecDeque,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .filter(|s| !s.starts_with("--"))
        .cloned()
}

/// Call before the `App` exists: installs the span profiler when `--trace` is given.
pub fn early_init(args: &[String]) {
    if !args.iter().any(|a| a == "--trace") {
        return;
    }
    #[cfg(feature = "trace")]
    trace::install();
    #[cfg(not(feature = "trace"))]
    eprintln!("perf: --trace needs a build with --features trace");
}

#[derive(Resource)]
pub struct Perf {
    frames: VecDeque<f32>,
    main_ms: VecDeque<f32>,
    frame_start: Option<Instant>,
    last_frame: Option<Instant>,
    overlay_on: bool,
    text_timer: f32,
    bench: Option<Bench>,
    // Samples for the benchmark (after warm-up).
    all_frames: Vec<f32>,
    all_main: Vec<f32>,
    gpu_samples: Vec<f32>,
    entities: usize,
    visible: usize,
    started: Instant,
}

struct Bench {
    seconds: f32,
    warmup: f32,
    name: String,
    walk: bool,
    measuring: bool,
    measure_start: Option<Instant>,
    ready_since: Option<Instant>,
    route_start: Option<Instant>,
    route_start_state: Option<BenchSnapshot>,
    route_end_state: Option<BenchSnapshot>,
    route_trace: Vec<(f32, BenchSnapshot)>,
    run_id: u128,
    readiness_wait_started: Instant,
}

/// Readiness and pose data supplied by the world update chain for moving benchmarks.
#[derive(Resource, Default)]
pub(crate) struct BenchWorld {
    pub(crate) enabled: bool,
    pub(crate) settled: bool,
    pub(crate) stable_frames: u32,
    pub(crate) player_position_cm: [f32; 3],
    pub(crate) player_yaw_rad: f32,
    pub(crate) camera_position_m: [f32; 3],
    pub(crate) camera_rotation_xyzw: [f32; 4],
}

#[derive(Clone, Copy)]
struct BenchSnapshot {
    player_position_cm: [f32; 3],
    player_yaw_rad: f32,
    camera_position_m: [f32; 3],
    camera_rotation_xyzw: [f32; 4],
    entities: usize,
    visible_meshes: usize,
    stable_frames: u32,
}

impl BenchWorld {
    fn snapshot(&self, entities: usize, visible_meshes: usize) -> BenchSnapshot {
        BenchSnapshot {
            player_position_cm: self.player_position_cm,
            player_yaw_rad: self.player_yaw_rad,
            camera_position_m: self.camera_position_m,
            camera_rotation_xyzw: self.camera_rotation_xyzw,
            entities,
            visible_meshes,
            stable_frames: self.stable_frames,
        }
    }
}

#[derive(Component)]
struct OverlayText;

pub struct PerfPlugin {
    pub args: Vec<String>,
}

impl Plugin for PerfPlugin {
    fn build(&self, app: &mut App) {
        let bench = flag(&self.args, "--bench").map(|s| Bench {
            seconds: s.parse().unwrap_or(20.0),
            warmup: flag(&self.args, "--bench-warmup")
                .and_then(|s| s.parse().ok())
                .unwrap_or(30.0),
            name: flag(&self.args, "--bench-name").unwrap_or_else(|| "bench".into()),
            walk: self.args.iter().any(|a| a == "--bench-walk"),
            measuring: false,
            measure_start: None,
            ready_since: None,
            route_start: None,
            route_start_state: None,
            route_end_state: None,
            route_trace: Vec::new(),
            run_id: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_millis()),
            readiness_wait_started: Instant::now(),
        });
        let gpu = self.args.iter().any(|a| a == "--bench-gpu");
        let bench_walk_enabled = bench.as_ref().is_some_and(|b| b.walk);
        if !app.is_plugin_added::<FrameTimeDiagnosticsPlugin>() {
            app.add_plugins(FrameTimeDiagnosticsPlugin::default());
        }
        if !app.is_plugin_added::<EntityCountDiagnosticsPlugin>() {
            app.add_plugins(EntityCountDiagnosticsPlugin::default());
        }
        if gpu && !app.is_plugin_added::<bevy::render::diagnostic::RenderDiagnosticsPlugin>() {
            app.add_plugins(bevy::render::diagnostic::RenderDiagnosticsPlugin);
        }
        let benching = bench.is_some();
        app.insert_resource(Perf {
            frames: VecDeque::new(),
            main_ms: VecDeque::new(),
            frame_start: None,
            last_frame: None,
            overlay_on: false,
            text_timer: 0.0,
            bench,
            all_frames: Vec::new(),
            all_main: Vec::new(),
            gpu_samples: Vec::new(),
            entities: 0,
            visible: 0,
            started: Instant::now(),
        })
        .insert_resource(BenchWorld {
            enabled: bench_walk_enabled,
            ..default()
        })
        .add_systems(Startup, spawn_overlay)
        .add_systems(PostStartup, shadow_preset)
        .add_systems(First, frame_begin)
        .add_systems(Last, frame_end)
        .add_systems(Update, (toggle_overlay, update_overlay, bench_drive));
        if benching {
            app.add_systems(PreUpdate, bench_walk.after(bevy::input::InputSystems));
            app.add_systems(Startup, no_vsync);
        }
    }
}

/// Shadow cost on the render thread grows with (casters x cascades): every distinct mesh in a
/// cascade is its own bin with its own buffer write and bind group. Measured at 1440p in Ardea:
/// 4 cascades / 160 m = 39 ms, 3 / 100 m = 19 ms, no shadows = 14 ms. Default to the middle
/// setting; `GR_CASC`, `GR_SHDIST` and `GR_SHFIRST` still override (they are read by `walk`
/// first, and this preset leaves the light alone when any of them is set).
fn shadow_preset(mut lights: Query<&mut bevy::light::CascadeShadowConfig>) {
    if ["GR_CASC", "GR_SHDIST", "GR_SHFIRST"]
        .iter()
        .any(|k| std::env::var_os(k).is_some())
    {
        return;
    }
    for mut cfg in &mut lights {
        *cfg = bevy::light::CascadeShadowConfigBuilder {
            num_cascades: 2,
            minimum_distance: 0.1,
            maximum_distance: 100.0,
            first_cascade_far_bound: 20.0,
            overlap_proportion: 0.2,
        }
        .build();
    }
}

fn no_vsync(mut w: Query<&mut Window, With<PrimaryWindow>>) {
    for mut w in &mut w {
        w.present_mode = PresentMode::AutoNoVsync;
    }
}

fn frame_begin(mut p: ResMut<Perf>) {
    let now = Instant::now();
    if let Some(last) = p.last_frame {
        let ms = (now - last).as_secs_f32() * 1000.0;
        p.frames.push_back(ms);
        if p.frames.len() > 240 {
            p.frames.pop_front();
        }
        if p.bench.as_ref().is_some_and(|b| b.measuring) {
            p.all_frames.push(ms);
        }
    }
    p.last_frame = Some(now);
    p.frame_start = Some(now);
}

fn frame_end(mut p: ResMut<Perf>) {
    if let Some(s) = p.frame_start {
        let ms = s.elapsed().as_secs_f32() * 1000.0;
        p.main_ms.push_back(ms);
        if p.main_ms.len() > 240 {
            p.main_ms.pop_front();
        }
        if p.bench.as_ref().is_some_and(|b| b.measuring) {
            p.all_main.push(ms);
        }
    }
}

fn spawn_overlay(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(8),
            top: px(8),
            padding: UiRect::all(px(6)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
        GlobalZIndex(10_000),
        Visibility::Hidden,
        OverlayRoot,
        children![(
            OverlayText,
            Text::new(""),
            TextFont {
                font_size: FontSize::Px(14.0),
                ..default()
            },
            TextColor(Color::srgb(0.7, 1.0, 0.7)),
        )],
    ));
}

#[derive(Component)]
struct OverlayRoot;

fn toggle_overlay(
    keys: Res<ButtonInput<KeyCode>>,
    mut p: ResMut<Perf>,
    mut root: Query<&mut Visibility, With<OverlayRoot>>,
) {
    if keys.just_pressed(KeyCode::F3) {
        p.overlay_on = !p.overlay_on;
        for mut v in &mut root {
            *v = if p.overlay_on {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
    }
    #[cfg(feature = "trace")]
    if keys.just_pressed(KeyCode::F4) {
        let frames = p.all_frames.len().max(p.frames.len()).max(1);
        match trace::write_table(frames) {
            Ok(path) => eprintln!("perf: span table written to {path}"),
            Err(e) => eprintln!("perf: span table: {e}"),
        }
    }
}

fn percentile(sorted: &[f32], q: f32) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    let i = ((sorted.len() - 1) as f32 * q).round() as usize;
    sorted[i]
}

fn gpu_ms(store: &DiagnosticsStore) -> Option<f32> {
    // Sum the GPU spans that have no enclosing span in the list (avoids double counting).
    let mut paths: Vec<(String, f64)> = store
        .iter()
        .filter_map(|d| {
            let s = d.path().to_string();
            (s.starts_with("render/") && s.ends_with("/elapsed_gpu"))
                .then(|| d.value().map(|v| (s, v)))
                .flatten()
        })
        .collect();
    if paths.is_empty() {
        return None;
    }
    paths.sort_by(|a, b| a.0.cmp(&b.0));
    let mut total = 0.0;
    let mut last: Option<String> = None;
    for (path, v) in &paths {
        let base = path.trim_end_matches("/elapsed_gpu");
        let nested = last
            .as_ref()
            .is_some_and(|l| base.starts_with(&format!("{l}/")));
        if !nested {
            total += v;
            last = Some(base.to_string());
        }
    }
    Some(total as f32)
}

#[allow(clippy::too_many_arguments)]
fn update_overlay(
    time: Res<Time<Real>>,
    mut p: ResMut<Perf>,
    world: Res<BenchWorld>,
    store: Res<DiagnosticsStore>,
    visible: Query<&ViewVisibility>,
    mut text: Query<&mut Text, With<OverlayText>>,
) {
    p.text_timer += time.delta_secs();
    let measuring = p.bench.as_ref().is_some_and(|b| b.measuring);
    let walk_context = world.settled && p.bench.as_ref().is_some_and(|b| b.walk);
    if (!p.overlay_on && !measuring && !walk_context) || p.text_timer < 0.25 {
        return;
    }
    p.text_timer = 0.0;
    p.entities = store
        .get(&EntityCountDiagnosticsPlugin::ENTITY_COUNT)
        .and_then(|d| d.value())
        .unwrap_or(0.0) as usize;
    p.visible = visible.iter().filter(|v| v.get()).count();
    let gpu = (p.overlay_on || measuring)
        .then(|| gpu_ms(&store))
        .flatten();
    if measuring && let Some(g) = gpu {
        p.gpu_samples.push(g);
    }
    if !p.overlay_on {
        return;
    }
    let mut sorted: Vec<f32> = p.frames.iter().copied().collect();
    sorted.sort_by(f32::total_cmp);
    let avg = sorted.iter().sum::<f32>() / sorted.len().max(1) as f32;
    let main = p.main_ms.iter().sum::<f32>() / p.main_ms.len().max(1) as f32;
    let mut s = format!(
        "{:.0} fps  frame {:.2} ms (p95 {:.2}, max {:.2})\nmain CPU {:.2} ms  wait {:.2} ms",
        1000.0 / avg.max(0.001),
        avg,
        percentile(&sorted, 0.95),
        sorted.last().copied().unwrap_or(0.0),
        main,
        (avg - main).max(0.0),
    );
    match gpu {
        Some(g) => s.push_str(&format!("\nGPU {g:.2} ms")),
        None => s.push_str("\nGPU n/a (--bench-gpu or GR_PROF=1)"),
    }
    s.push_str(&format!(
        "\nentities {}  visible meshes {}",
        p.entities, p.visible
    ));
    if let Ok(mut t) = text.single_mut() {
        t.0 = s;
    }
}

/// Scripted walking for `--bench-walk`: forward for 5 s, back for 5 s from route start.
fn bench_walk(mut keys: ResMut<ButtonInput<KeyCode>>, p: Res<Perf>) {
    let Some(b) = &p.bench else { return };
    if !b.walk {
        return;
    }
    let Some(route_start) = b.route_start else {
        keys.release(KeyCode::KeyW);
        keys.release(KeyCode::KeyS);
        return;
    };
    let phase = (route_start.elapsed().as_secs_f32() / 5.0) as u32 % 2;
    if phase == 0 {
        keys.release(KeyCode::KeyS);
        keys.press(KeyCode::KeyW);
    } else {
        keys.release(KeyCode::KeyW);
        keys.press(KeyCode::KeyS);
    }
}

fn walk_sample_due(bench: &Bench, elapsed: f32) -> bool {
    bench.walk && elapsed.floor() as usize >= bench.route_trace.len()
}

fn bench_drive(mut p: ResMut<Perf>, world: Res<BenchWorld>, mut exit: MessageWriter<AppExit>) {
    let elapsed = p.started.elapsed().as_secs_f32();
    let started = p.started;
    let entities = p.entities;
    let visible = p.visible;
    let Some(b) = p.bench.as_mut() else { return };
    if !b.measuring {
        // A walk run waits for actual settled world/player readiness, then applies the same
        // warm-up duration. Idle benchmarks retain their original startup-relative timing.
        let measure_at = if b.walk {
            if world.settled {
                b.readiness_wait_started = Instant::now();
                let ready_since = *b.ready_since.get_or_insert_with(Instant::now);
                if ready_since.elapsed().as_secs_f32() >= b.warmup {
                    let now = Instant::now();
                    b.route_start.get_or_insert(now);
                    b.route_start_state
                        .get_or_insert_with(|| world.snapshot(entities, visible));
                    Some(now)
                } else {
                    None
                }
            } else {
                b.ready_since = None;
                None
            }
        } else if elapsed >= b.warmup {
            Some(Instant::now())
        } else {
            None
        };
        if let Some(start) = measure_at {
            b.measuring = true;
            b.measure_start = Some(start);
            #[cfg(feature = "trace")]
            trace::reset();
            eprintln!(
                "perf: warm-up done after {elapsed:.1} s, measuring {} s{}",
                b.seconds,
                if b.walk { " (world settled)" } else { "" },
            );
        } else if b.walk
            && !world.settled
            && b.readiness_wait_started.elapsed().as_secs_f32() >= 300.0
        {
            eprintln!(
                "perf: --bench-walk timed out after {:.1} s waiting for settled streaming and placed player (stable frames {})",
                b.readiness_wait_started.elapsed().as_secs_f32(),
                world.stable_frames
            );
            exit.write(AppExit::error());
        }
        return;
    }
    let ran = b.measure_start.map_or(0.0, |s| s.elapsed().as_secs_f32());
    if walk_sample_due(b, ran) {
        b.route_trace.push((ran, world.snapshot(entities, visible)));
    }
    if ran < b.seconds {
        return;
    }
    let name = b.name.clone();
    let seconds = ran;
    let walk = b.walk;
    if walk {
        b.route_end_state = Some(world.snapshot(entities, visible));
    }
    let run_id = b.run_id;
    let warmup_after_ready = walk.then_some(b.warmup);
    let ready_wait = b
        .ready_since
        .map(|ready| ready.duration_since(started).as_secs_f32());
    let route_seconds = b.route_start.map(|start| start.elapsed().as_secs_f32());
    let route_start_state = b.route_start_state;
    let route_end_state = b.route_end_state;
    let route_trace = b.route_trace.clone();
    let mut sorted = p.all_frames.clone();
    sorted.sort_by(f32::total_cmp);
    let mut main = p.all_main.clone();
    main.sort_by(f32::total_cmp);
    let n = sorted.len().max(1) as f32;
    let avg = sorted.iter().sum::<f32>() / n;
    let main_avg = main.iter().sum::<f32>() / main.len().max(1) as f32;
    let gpu_avg = (!p.gpu_samples.is_empty())
        .then(|| p.gpu_samples.iter().sum::<f32>() / p.gpu_samples.len() as f32);
    #[allow(unused_mut)]
    let mut json = serde_json::json!({
        "name": name,
        "walk": walk,
        "sample_id": format!("{}-{run_id}", name),
        "walk_route": walk.then_some("forward_5s_back_5s"),
        "world_ready_wait_seconds": ready_wait,
        "warmup_after_ready_seconds": warmup_after_ready,
        "world_stable_frames_at_end": world.stable_frames,
        "route_elapsed_seconds_at_end": route_seconds,
        "route_start": route_start_state.map(|s| serde_json::json!({
            "player_position_cm": s.player_position_cm,
            "player_yaw_rad": s.player_yaw_rad,
            "camera_position_m": s.camera_position_m,
            "camera_rotation_xyzw": s.camera_rotation_xyzw,
            "entities": s.entities,
            "visible_meshes": s.visible_meshes,
            "stable_frames": s.stable_frames,
        })),
        "route_end": route_end_state.map(|s| serde_json::json!({
            "player_position_cm": s.player_position_cm,
            "player_yaw_rad": s.player_yaw_rad,
            "camera_position_m": s.camera_position_m,
            "camera_rotation_xyzw": s.camera_rotation_xyzw,
            "entities": s.entities,
            "visible_meshes": s.visible_meshes,
            "stable_frames": s.stable_frames,
        })),
        "route_trace_1hz": route_trace.iter().map(|(time, s)| serde_json::json!({
            "seconds": time,
            "player_position_cm": s.player_position_cm,
            "player_yaw_rad": s.player_yaw_rad,
            "camera_position_m": s.camera_position_m,
            "camera_rotation_xyzw": s.camera_rotation_xyzw,
            "entities": s.entities,
            "visible_meshes": s.visible_meshes,
        })).collect::<Vec<_>>(),
        "seconds": seconds,
        "frames": sorted.len(),
        "avg_ms": avg,
        "p50_ms": percentile(&sorted, 0.5),
        "p95_ms": percentile(&sorted, 0.95),
        "p99_ms": percentile(&sorted, 0.99),
        "max_ms": sorted.last().copied().unwrap_or(0.0),
        "min_ms": sorted.first().copied().unwrap_or(0.0),
        "fps": 1000.0 / avg.max(0.001),
        "main_cpu_avg_ms": main_avg,
        "main_cpu_p95_ms": percentile(&main, 0.95),
        "gpu_avg_ms": gpu_avg,
        "entities": p.entities,
        "visible_meshes": p.visible,
        "args": std::env::args().collect::<Vec<_>>(),
    });
    #[cfg(feature = "trace")]
    {
        json["top_spans_per_frame"] = trace::top_json(sorted.len().max(1), 40);
        if let Ok(path) = trace::write_table(sorted.len().max(1)) {
            eprintln!("perf: span table written to {path}");
        }
    }
    let unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let dir = std::path::Path::new(".local/bench");
    let _ = std::fs::create_dir_all(dir);
    let path = dir.join(format!("{name}-{unix}.json"));
    match std::fs::write(
        &path,
        serde_json::to_string_pretty(&json).unwrap_or_default(),
    ) {
        Ok(()) => eprintln!("perf: wrote {}", path.display()),
        Err(e) => eprintln!("perf: cannot write {}: {e}", path.display()),
    }
    println!("{}", serde_json::to_string(&json).unwrap_or_default());
    exit.write(AppExit::Success);
}

#[cfg(feature = "trace")]
mod trace {
    use bevy::log::tracing::{
        self, Subscriber,
        field::{Field, Visit},
        span,
    };
    use bevy::log::tracing_subscriber::{
        Layer, filter::LevelFilter, layer::Context, prelude::*, registry::LookupSpan,
    };
    use std::{
        collections::HashMap,
        sync::{
            Arc, Mutex, OnceLock,
            atomic::{AtomicU64, Ordering},
        },
        time::Instant,
    };

    pub struct Slot {
        name: String,
        total_ns: AtomicU64,
        count: AtomicU64,
        max_ns: AtomicU64,
    }

    fn registry() -> &'static Mutex<HashMap<String, Arc<Slot>>> {
        static R: OnceLock<Mutex<HashMap<String, Arc<Slot>>>> = OnceLock::new();
        R.get_or_init(|| Mutex::new(HashMap::new()))
    }

    struct Ext {
        slot: Arc<Slot>,
        started: Option<Instant>,
        depth: u32,
    }

    struct NameVisitor(Option<String>);
    impl Visit for NameVisitor {
        fn record_str(&mut self, f: &Field, v: &str) {
            if f.name() == "name" {
                self.0 = Some(v.to_string());
            }
        }
        fn record_debug(&mut self, f: &Field, v: &dyn std::fmt::Debug) {
            if f.name() == "name" && self.0.is_none() {
                self.0 = Some(format!("{v:?}").trim_matches('"').to_string());
            }
        }
    }

    struct PerfLayer;

    impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for PerfLayer {
        fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: Context<'_, S>) {
            let mut v = NameVisitor(None);
            attrs.record(&mut v);
            let key = match v.0 {
                Some(n) => format!("{}:{}", attrs.metadata().name(), n),
                None => attrs.metadata().name().to_string(),
            };
            let slot = {
                let mut r = registry().lock().expect("trace registry");
                r.entry(key.clone())
                    .or_insert_with(|| {
                        Arc::new(Slot {
                            name: key,
                            total_ns: AtomicU64::new(0),
                            count: AtomicU64::new(0),
                            max_ns: AtomicU64::new(0),
                        })
                    })
                    .clone()
            };
            if let Some(span) = ctx.span(id) {
                span.extensions_mut().insert(Ext {
                    slot,
                    started: None,
                    depth: 0,
                });
            }
        }

        fn on_enter(&self, id: &span::Id, ctx: Context<'_, S>) {
            if let Some(span) = ctx.span(id)
                && let Some(e) = span.extensions_mut().get_mut::<Ext>()
            {
                if e.depth == 0 {
                    e.started = Some(Instant::now());
                }
                e.depth += 1;
            }
        }

        fn on_exit(&self, id: &span::Id, ctx: Context<'_, S>) {
            if let Some(span) = ctx.span(id)
                && let Some(e) = span.extensions_mut().get_mut::<Ext>()
            {
                e.depth = e.depth.saturating_sub(1);
                if e.depth == 0
                    && let Some(s) = e.started.take()
                {
                    let ns = s.elapsed().as_nanos() as u64;
                    e.slot.total_ns.fetch_add(ns, Ordering::Relaxed);
                    e.slot.count.fetch_add(1, Ordering::Relaxed);
                    e.slot.max_ns.fetch_max(ns, Ordering::Relaxed);
                }
            }
        }
    }

    pub fn install() {
        let sub = tracing_subscriber_registry();
        let _ = tracing::subscriber::set_global_default(sub);
    }

    fn tracing_subscriber_registry() -> impl Subscriber + Send + Sync {
        bevy::log::tracing_subscriber::registry().with(PerfLayer.with_filter(LevelFilter::INFO))
    }

    pub fn reset() {
        for s in registry().lock().expect("trace registry").values() {
            s.total_ns.store(0, Ordering::Relaxed);
            s.count.store(0, Ordering::Relaxed);
            s.max_ns.store(0, Ordering::Relaxed);
        }
    }

    /// (name, total ms, calls, max ms) sorted by total.
    fn snapshot() -> Vec<(String, f64, u64, f64)> {
        let mut v: Vec<_> = registry()
            .lock()
            .expect("trace registry")
            .values()
            .map(|s| {
                (
                    s.name.clone(),
                    s.total_ns.load(Ordering::Relaxed) as f64 / 1.0e6,
                    s.count.load(Ordering::Relaxed),
                    s.max_ns.load(Ordering::Relaxed) as f64 / 1.0e6,
                )
            })
            .filter(|t| t.2 > 0)
            .collect();
        v.sort_by(|a, b| b.1.total_cmp(&a.1));
        v
    }

    pub fn top_json(frames: usize, n: usize) -> serde_json::Value {
        let f = frames as f64;
        serde_json::Value::Array(
            snapshot()
                .into_iter()
                .take(n)
                .map(|(name, total, calls, max)| {
                    serde_json::json!({
                        "span": name,
                        "ms_per_frame": total / f,
                        "calls_per_frame": calls as f64 / f,
                        "max_ms": max,
                    })
                })
                .collect(),
        )
    }

    pub fn write_table(frames: usize) -> Result<String, String> {
        let f = frames as f64;
        let mut out = format!(
            "frames {frames}\n{:>10} {:>9} {:>9}  span (inclusive, summed over threads)\n",
            "ms/frame", "calls/fr", "max ms"
        );
        let all = snapshot();
        for (name, total, calls, max) in all.iter().take(60) {
            out.push_str(&format!(
                "{:>10.4} {:>9.2} {:>9.3}  {name}\n",
                total / f,
                *calls as f64 / f,
                max
            ));
        }
        out.push_str("\n-- systems only --\n");
        for (name, total, calls, max) in all.iter().filter(|t| t.0.starts_with("system:")).take(90)
        {
            out.push_str(&format!(
                "{:>10.4} {:>9.2} {:>9.3}  {name}\n",
                total / f,
                *calls as f64 / f,
                max
            ));
        }
        let unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let _ = std::fs::create_dir_all(".local/bench");
        let path = format!(".local/bench/trace-{unix}.txt");
        std::fs::write(&path, out).map_err(|e| e.to_string())?;
        Ok(path)
    }
}
