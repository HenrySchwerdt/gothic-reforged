//! Grows the shared navigation mesh from the collision world around the hero (and every NPC
//! spawn) and hands it to the simulation's `PlayWorld`, so chasing and routines follow real
//! paths with avoidance instead of straight lines (`gothic-nav`).
use super::NpcActor;
use crate::walk::{Collision, Player, Stream, tile_chunk_id};
use bevy::prelude::*;
use gothic_nav::{NavMesh, NavParams, SharedNav};
use gothic_physics::crowd::{Body, separate};
use gothic_runtime::CoreRes;
use gothic_sim::types::ActorId;
use std::collections::HashSet;

/// The shared mesh (the same handle lives in `PlayWorld::nav`).
#[derive(Resource)]
pub struct NavRes(pub SharedNav);

/// Decodes the authored navigation zones and corridors of every world document on a worker
/// thread and installs the original-graph `FindWay` (E39-E43) in the shared handle when done.
/// Positions go through the same mirrored, origin-relative map as the NPC placements.
fn spawn_graph_build(nav: SharedNav, data_dir: std::path::PathBuf) {
    let _ = std::thread::Builder::new()
        .name("nav-graph".into())
        .spawn(move || {
            let t = std::time::Instant::now();
            let Ok(mut vfs) = gothic_formats::vfs::VirtualFileSystem::open_data_dir(&data_dir)
            else {
                eprintln!("play: nav graph: cannot open the data directory");
                return;
            };
            let slots: Vec<_> = vfs
                .iter()
                .filter(|(p, _)| {
                    let l = p.to_ascii_lowercase();
                    // The whole world: every authored layer can hold zones and corridors
                    // (gCNavZone_PS / gCNavPath_PS); unlisted level meshes carry none.
                    (l.ends_with(".node") || l.ends_with(".lrentdat"))
                        && !l.contains("levelmesh")
                        && !is_static_cell(&l)
                })
                .map(|(p, s)| (p.to_owned(), s))
                .collect();
            let mut scene = gothic_nav::NavScene::default();
            for (path, slot) in &slots {
                match vfs.read(*slot) {
                    Ok(bytes) => {
                        if let Err(e) = scene.add_document(&bytes, false) {
                            eprintln!("play: nav graph: {path}: {e:#}");
                        }
                    }
                    Err(e) => eprintln!("play: nav graph: {path}: {e:#}"),
                }
            }
            let origin = crate::world_loader::Origin(crate::walk::ORIGIN);
            let graph = gothic_nav::NavGraph::build(&scene, &|p| origin.point(p));
            eprintln!(
                "play: nav graph: {} zones, {} corridors ({} dangling) from {} documents in {} ms",
                graph.zone_count(),
                graph.path_count(),
                graph.dangling_paths,
                slots.len(),
                t.elapsed().as_millis()
            );
            nav.set_graph(std::sync::Arc::new(graph));
        });
}

/// `x95000y0z-15000.node`: a static-geometry cell, never holds navigation property sets.
fn is_static_cell(path: &str) -> bool {
    let stem = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let b = stem.as_bytes();
    b.len() > 4 && b[0] == b'x' && (b[1].is_ascii_digit() || b[1] == b'-') && stem.contains('y')
}

pub fn register(app: &mut App, data_dir: &std::path::Path) {
    let shared = SharedNav::new(NavMesh::new(NavParams::default()));
    spawn_graph_build(shared.clone(), data_dir.to_path_buf());
    app.insert_resource(NavRes(shared))
        .add_systems(Update, grow)
        .add_systems(
            Update,
            hero_vs_npcs
                .after(crate::walk::player_motion)
                .before(crate::walk::update_camera),
        );
}

/// The hero is deflected by standing NPCs (V: `CanBePushedWhileIdle` is false on every shipped
/// template, so the mover gives way). Dead characters do not block.
fn hero_vs_npcs(mut player: ResMut<Player>, core: Res<CoreRes>, collision: Res<Collision>) {
    let hero = player.ctrl.position;
    let radius = player.ctrl.config.radius;
    let height = player.ctrl.config.height;
    let mut bodies = vec![Body::new(hero, radius, height)];
    for (id, (p, _)) in &core.0.sim.world.actors {
        if *id == core.0.hero || (p.x - hero.x).hypot(p.z - hero.z) > 300.0 {
            continue;
        }
        if core.0.sim.actor(*id).hp <= 0 {
            continue;
        }
        let mut b = Body::new(Vec3::new(p.x, p.y, p.z), 35.3, 200.0);
        b.moving = false;
        bodies.push(b);
    }
    if bodies.len() < 2 {
        return;
    }
    let delta = separate(&bodies, 2)[0];
    if delta.length_squared() > 1.0e-4 {
        player.ctrl.nudge(&collision.0, delta);
    }
}

/// Largest radius (cm, <= 9000) around `centre` whose ring lies on loaded collision tiles.
fn loaded_radius(collision: &Collision, stream: &Stream, centre: Vec3) -> Option<f32> {
    let ok = |p: Vec3| {
        let c = stream.settings.cell_at(p);
        collision.0.has_chunk(tile_chunk_id((c.x, c.z)))
    };
    let mut r = 9000.0;
    while r >= 1500.0 {
        let probe = r + 600.0;
        let ring = (0..16).all(|i| {
            let a = i as f32 * std::f32::consts::TAU / 16.0;
            ok(centre + Vec3::new(a.cos() * probe, 0.0, a.sin() * probe))
        });
        if ring && ok(centre) {
            return Some(r);
        }
        r -= 750.0;
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn grow(
    nav: Res<NavRes>,
    collision: Res<Collision>,
    player: Res<Player>,
    stream: Res<Stream>,
    time: Res<Time>,
    mut core: ResMut<CoreRes>,
    npcs: Query<&NpcActor>,
    mut clock: Local<f32>,
    mut linked: Local<bool>,
    mut seeded: Local<HashSet<ActorId>>,
) {
    if !*linked {
        core.0.sim.world.nav = Some(nav.0.clone());
        *linked = true;
    }
    *clock += time.delta_secs();
    if *clock < 0.1 {
        return;
    }
    *clock = 0.0;
    let hero = player.ctrl.position;
    if !player.ctrl.grounded {
        return;
    }
    let Some(radius) = loaded_radius(&collision, &stream, hero) else {
        return;
    };
    let Ok(mut mesh) = nav.0.0.write() else {
        return;
    };
    mesh.add_seed(&collision.0, hero);
    // NPC spawn points seed their own islands (interiors, roofs the hero never walked to).
    for npc in &npcs {
        if seeded.contains(&npc.0) {
            continue;
        }
        let Some((p, _)) = core.0.sim.world.actors.get(&npc.0).copied() else {
            continue;
        };
        let at = Vec3::new(p.x, p.y, p.z);
        let c = stream.settings.cell_at(at);
        if collision.0.has_chunk(tile_chunk_id((c.x, c.z))) {
            mesh.add_seed(&collision.0, at);
            seeded.insert(npc.0);
        }
    }
    mesh.extend(
        &collision.0,
        Some((Vec2::new(hero.x, hero.z), radius)),
        1500,
    );
}
