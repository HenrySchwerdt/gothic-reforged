//! The effect data of the installed game: effect map, particle templates, Ardea effect sites and
//! a simulated torch. Skipped when the game data is not present (set `GOTHIC3_DATA` to the
//! `Data` folder to point elsewhere).
use bevy::math::{Mat4, Vec3};
use gothic_formats::vfs::VirtualFileSystem;
use gothic_fx::draw::{View, emitter_quads};
use gothic_fx::efm::{CommandKind, EffectMap};
use gothic_fx::sim::Emitter;
use gothic_fx::templates::FxTemplates;
use gothic_fx::world::{ScanArea, scan};
use std::path::PathBuf;
use std::sync::Arc;

fn data_dir() -> Option<PathBuf> {
    let p = std::env::var_os("GOTHIC3_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"D:\SteamLibrary\steamapps\common\Gothic 3\Data"));
    p.is_dir().then_some(p)
}

#[test]
fn effect_map_templates_and_a_torch() {
    let Some(dir) = data_dir() else {
        eprintln!("game data not found, skipped");
        return;
    };
    let mut vfs = VirtualFileSystem::open_data_dir(&dir).expect("data dir");
    let templates = FxTemplates::load(&mut vfs).expect("templates");
    assert!(
        templates.len() > 400,
        "{} particle templates",
        templates.len()
    );
    let bytes = vfs
        .resolve_name("g3.efm")
        .and_then(|s| vfs.read(s).ok())
        .expect("g3.efm");
    let map = EffectMap::parse(&bytes).expect("effect map");
    assert_eq!(map.len(), 595);

    // Every spawn command either resolves to a particle template or names another kind of
    // entity (creatures, lights); the large majority are particles.
    let (mut spawns, mut resolved) = (0, 0);
    for name in map.names() {
        for c in &map.lookup(name).expect("sequence").commands {
            if let CommandKind::SpawnEntity {
                template: Some(g), ..
            } = &c.kind
            {
                spawns += 1;
                resolved += usize::from(templates.by_guid(g).is_some());
            }
        }
    }
    assert!(resolved * 100 / spawns >= 80, "{resolved} of {spawns}");

    // The torch: three spawn commands, the flame is an additive 6x2 sheet of 12 frames.
    let torch = map.lookup("eff_item_torch_01").expect("torch effect");
    let flame = torch
        .commands
        .iter()
        .find_map(|c| match &c.kind {
            CommandKind::SpawnEntity {
                template: Some(g), ..
            } => templates
                .by_guid(g)
                .filter(|s| s.name == "VFX_Env_Fire_Torch_01"),
            _ => None,
        })
        .expect("torch flame template");
    let params = flame.emitters[0].params.clone();
    assert_eq!(params.texture, "VFX_Env_Torch_01.tga");
    assert_eq!(params.frame_count(), 12);
    let mut emitter = Emitter::new(Arc::new(params.mirrored_x()), 7);
    emitter.set_transform(Mat4::IDENTITY);
    for _ in 0..240 {
        emitter.update(1.0 / 60.0);
    }
    let mut quads = Vec::new();
    emitter_quads(
        &emitter,
        &View::new(Vec3::new(0.0, 100.0, 400.0), bevy::math::Quat::IDENTITY),
        &mut quads,
    );
    assert!(!quads.is_empty());
    // The flame stays near the emitter.
    assert!(quads.iter().all(|q| q.corners[0].length() < 400.0));
}

#[test]
fn ardea_has_torches_candles_and_a_campfire() {
    let Some(dir) = data_dir() else {
        eprintln!("game data not found, skipped");
        return;
    };
    let mut vfs = VirtualFileSystem::open_data_dir(&dir).expect("data dir");
    let world = scan(
        &mut vfs,
        &ScanArea {
            center: Vec3::new(92_400.0, 0.0, -11_000.0),
            radius: 20_000.0,
            document: &|p| p.contains("ardea") || p.contains("sysdyn"),
        },
    );
    let count = |name: &str| world.effects.iter().filter(|e| e.effect == name).count();
    assert!(count("eff_item_torch_01") >= 10);
    assert!(count("eff_env_fire_candle_01") >= 20);
    assert!(count("eff_env_fire_campfire_02") >= 5);
    assert!(
        world
            .emitters
            .iter()
            .any(|e| e.name == "VFX_Env_Fire_Campfire_01")
    );
}
