//! World sound emitters (`eCAudioEmitter_PS` on world entities): the property sets of the world
//! documents become `AudioEmitterPs` entities inside a radius around the hero; the audio plugin
//! runs the ported emitter state machine (`eCAudioEmitter_PS::OnProcess`) on them.
//!
//! Cached in `.local/world_emitters.json` like the trigger table.
use crate::walk::{ORIGIN, Player};
use crate::world_loader::Origin;
use bevy::prelude::*;
use gothic_audio::AudioEmitterPs;
use gothic_audio::emitter::{EmitterConfig, SpawningMode};
use gothic_formats::propstream::{DecodeStats, PropValue, PropertySet};
use gothic_formats::vfs::VirtualFileSystem;
use gothic_formats::world_node::WorldDocument;
use std::collections::HashMap;
use std::path::Path;

const CACHE: &str = ".local/world_emitters.json";
const CLASS: &str = "eCAudioEmitter_PS";
/// Emitters are live inside this radius of the hero (cm) and dropped beyond the second one.
const SHOW: f32 = 30_000.0;
const DROP: f32 = 36_000.0;
const CM: f32 = 0.01;

pub struct EmitterSrc {
    pub name: String,
    pub pos_cm: Vec3,
    pub config: EmitterConfig,
}

#[derive(Resource, Default)]
pub struct WorldEmitters {
    pub src: Vec<EmitterSrc>,
    live: HashMap<usize, Entity>,
    last: f32,
}

impl WorldEmitters {
    pub fn new(src: Vec<EmitterSrc>) -> Self {
        Self {
            src,
            ..Default::default()
        }
    }

    pub fn live_count(&self) -> usize {
        self.live.len()
    }
}

fn config_of(s: &PropertySet) -> EmitterConfig {
    let d = EmitterConfig::default();
    let b = |n: &str, def: bool| match s.get(n) {
        Some(PropValue::Bool(v)) => *v,
        _ => def,
    };
    let f = |n: &str, def: f32| match s.get(n) {
        Some(PropValue::F32(v)) => *v,
        _ => def,
    };
    let r = |n: &str, def: [f32; 2]| match s.get(n) {
        Some(PropValue::Range1(v)) => *v,
        _ => def,
    };
    let mode = match s.get("SpawningMode") {
        Some(PropValue::Enum { value, .. }) => SpawningMode::from_index(*value),
        _ => d.mode,
    };
    EmitterConfig {
        sample: match s.get("Sound") {
            Some(PropValue::Str(v)) => v.clone(),
            _ => String::new(),
        },
        enabled: b("Enabled", d.enabled),
        auto_reset: b("AutoReset", d.auto_reset),
        auto_kill: b("AutoKill", d.auto_kill),
        use_stream: b("UseStream", d.use_stream),
        volume: f("Volume", d.volume),
        min_distance: f("MinDistance", d.min_distance),
        max_distance: f("MaxDistance", d.max_distance),
        mode,
        seconds_between_repeats: r("SecondsBetweenRepeats", d.seconds_between_repeats),
        repeat_probability: r("RepeatProbability", d.repeat_probability),
        use_max_repeats: b("UseMaxRepeats", d.use_max_repeats),
        max_repeats: r("MaxNumRepeats", d.max_repeats),
    }
}

fn mode_index(m: SpawningMode) -> i64 {
    match m {
        SpawningMode::Once => 0,
        SpawningMode::Loop => 1,
        SpawningMode::Repeat => 2,
    }
}

fn to_json(c: &EmitterConfig) -> serde_json::Value {
    serde_json::json!({
        "sample": c.sample, "enabled": c.enabled, "auto_reset": c.auto_reset,
        "auto_kill": c.auto_kill, "use_stream": c.use_stream, "volume": c.volume,
        "min": c.min_distance, "max": c.max_distance, "mode": mode_index(c.mode),
        "between": c.seconds_between_repeats, "prob": c.repeat_probability,
        "use_max": c.use_max_repeats, "max_repeats": c.max_repeats,
    })
}

fn from_json(v: &serde_json::Value) -> EmitterConfig {
    let f = |k: &str| v[k].as_f64().unwrap_or(0.0) as f32;
    let b = |k: &str| v[k].as_bool().unwrap_or(false);
    let r = |k: &str| {
        [
            v[k][0].as_f64().unwrap_or(0.0) as f32,
            v[k][1].as_f64().unwrap_or(0.0) as f32,
        ]
    };
    EmitterConfig {
        sample: v["sample"].as_str().unwrap_or("").to_owned(),
        enabled: b("enabled"),
        auto_reset: b("auto_reset"),
        auto_kill: b("auto_kill"),
        use_stream: b("use_stream"),
        volume: f("volume"),
        min_distance: f("min"),
        max_distance: f("max"),
        mode: SpawningMode::from_index(v["mode"].as_i64().unwrap_or(2) as i32),
        seconds_between_repeats: r("between"),
        repeat_probability: r("prob"),
        use_max_repeats: b("use_max"),
        max_repeats: r("max_repeats"),
    }
}

/// Every emitter of the world documents, from the cache or the installation.
pub fn load(data_dir: &Path) -> Vec<EmitterSrc> {
    let origin = Origin(ORIGIN);
    let rows: Vec<serde_json::Value> = std::fs::read(CACHE)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_else(|| {
            let mut rows = Vec::new();
            let Ok(mut vfs) = VirtualFileSystem::open_data_dir(data_dir) else {
                return rows;
            };
            let slots: Vec<_> = vfs
                .iter()
                .filter(|(p, _)| {
                    let l = p.to_ascii_lowercase();
                    l.ends_with(".node") || l.ends_with(".lrentdat")
                })
                .map(|(p, s)| (p.to_owned(), s))
                .collect();
            for (_path, slot) in slots {
                let Ok(bytes) = vfs.read(slot) else { continue };
                if !bytes.windows(CLASS.len()).any(|w| w == CLASS.as_bytes()) {
                    continue;
                }
                let mut stats = DecodeStats::default();
                let Ok((doc, sets)) =
                    WorldDocument::parse_with_properties(&bytes, &|c| c == CLASS, &mut stats)
                else {
                    continue;
                };
                for (n, e) in doc.entities.iter().enumerate() {
                    let Some(set) = sets
                        .get(n)
                        .and_then(|s| s.iter().find(|s| s.class == CLASS))
                    else {
                        continue;
                    };
                    let cfg = config_of(set);
                    if cfg.sample.is_empty() {
                        continue;
                    }
                    let m = e.world_matrix[3];
                    let mut row = to_json(&cfg);
                    row["entity"] = e.name.clone().into();
                    row["position"] = serde_json::json!([m[0], m[1], m[2]]);
                    rows.push(row);
                }
            }
            eprintln!("play: {} {CLASS} sets read", rows.len());
            if let Ok(json) = serde_json::to_vec(&rows) {
                let _ = std::fs::write(CACHE, json);
            }
            rows
        });
    rows.iter()
        .map(|r| {
            let g = |k: usize| r["position"][k].as_f64().unwrap_or(0.0) as f32;
            EmitterSrc {
                name: r["entity"].as_str().unwrap_or("").to_owned(),
                pos_cm: origin.point(Vec3::new(g(0), g(1), g(2))),
                config: from_json(r),
            }
        })
        .collect()
}

pub fn register(app: &mut App) {
    app.add_systems(Update, drive_emitters);
}

/// Creates the emitter entities near the hero and drops the far ones.
fn drive_emitters(
    time: Res<Time>,
    player: Res<Player>,
    mut we: ResMut<WorldEmitters>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs();
    if now - we.last < 0.5 || we.src.is_empty() {
        return;
    }
    we.last = now;
    let hero = player.ctrl.position;
    let far = |p: Vec3, limit: f32| (p.x - hero.x).abs().max((p.z - hero.z).abs()) > limit;
    let we = &mut *we;
    let gone: Vec<usize> = we
        .live
        .keys()
        .copied()
        .filter(|i| far(we.src[*i].pos_cm, DROP))
        .collect();
    for i in gone {
        if let Some(e) = we.live.remove(&i) {
            commands.entity(e).despawn();
        }
    }
    for (i, s) in we.src.iter().enumerate() {
        if we.live.contains_key(&i) || far(s.pos_cm, SHOW) {
            continue;
        }
        let e = commands
            .spawn((
                Name::new(format!("emitter {}", s.name)),
                Transform::from_translation(s.pos_cm * CM),
                AudioEmitterPs::new(s.config.clone()),
            ))
            .id();
        we.live.insert(i, e);
    }
}
